use super::source_archive::{self, sha256_file};
use super::{Target, TargetContext};
use crate::config::{Build, Repository};
use crate::logger::{Color, Logger, colorize};
use crate::publish::store::Store;
use crate::publish::{index, repo_files};
use crate::release::fail_fast_or_continue;
use crate::template::{Template, Var};
use anyhow::{Context, Result};
use formula::BottleMeta;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

mod brew;
mod formula;
mod tap;

pub const ID: &str = "macos";
const NAME: &str = "macOS (Homebrew)";
const FAMILY: &str = "macOS";
const PACKAGE_TYPE: &str = "homebrew";
const TAP_USER: &str = "omnipackage";
const META_FILE: &str = "bottle-meta.json";
const TAP_ANCHOR: &str = "tap.git/info/refs";

pub struct MacTarget {
    build: Build,
}

struct Layout {
    root: PathBuf,
    output: PathBuf,
    stage: PathBuf,
    tap_work: PathBuf,
    tap_bare: PathBuf,
    log: PathBuf,
}

impl MacTarget {
    pub fn new(build: Build) -> Self {
        Self { build }
    }

    fn layout(&self, ctx: &TargetContext) -> Layout {
        let root = ctx.job.build_dir.join(format!("{}-{}", self.build.package_name, ID));
        Layout {
            output: root.join("output"),
            stage: root.join("src"),
            tap_work: root.join("tap"),
            tap_bare: root.join("tap.git"),
            log: root.join("homebrew.log"),
            root,
        }
    }

    fn tap_name(&self) -> String {
        format!("{TAP_USER}/{}", self.build.package_name)
    }

    fn formula_ref(&self) -> String {
        format!("{}/{}", self.tap_name(), self.build.package_name)
    }

    fn formula_file(&self) -> String {
        format!("Formula/{}.rb", self.build.package_name)
    }

    fn source_folder_name(&self, ctx: &TargetContext) -> String {
        format!("{}-{}", self.build.package_name, ctx.job.job_variables.version)
    }

    fn render_formula(&self, ctx: &TargetContext, url: &str, sha256: &str) -> Result<String> {
        let template = &self.build.homebrew.as_ref().context("homebrew config is missing")?.formula_template;
        let mut vars: HashMap<String, Var> = ctx.job.job_variables.to_template_vars();
        vars.extend(self.build.to_template_vars());
        vars.insert("formula_class".to_string(), formula::formula_class(&self.build.package_name).into());
        vars.insert("source_folder_name".to_string(), self.source_folder_name(ctx).into());
        vars.insert("url".to_string(), url.into());
        vars.insert("sha256".to_string(), sha256.into());
        Template::from_file(ctx.job.source_dir.join(template))?.render(vars)
    }

    fn run_build(&self, ctx: &TargetContext) -> Result<()> {
        if std::env::consts::OS != "macos" {
            anyhow::bail!("{} packages can only be built on a macOS host (this is {})", ID, std::env::consts::OS);
        }
        if self.build.before_build_script.is_some() {
            Logger::new().warn("before_build_script is ignored for macos: put build steps into the formula's install block");
        }
        if !ctx.job.job_variables.secrets.is_empty() {
            Logger::new().warn("secrets are not passed to Homebrew builds");
        }

        let layout = self.layout(ctx);
        if layout.root.exists() {
            std::fs::remove_dir_all(&layout.root).with_context(|| format!("cannot clean build dir {}", layout.root.display()))?;
        }
        std::fs::create_dir_all(&layout.output)?;

        let archive = source_archive::create(&ctx.job.source_dir, &ctx.job.ignore_source_files, &self.source_folder_name(ctx), &layout.stage, &layout.output)?;
        let formula = self.render_formula(ctx, &format!("file://{}", archive.path.display()), &archive.sha256)?;

        tap::init_bare(&layout.tap_bare)?;
        tap::clone(&layout.tap_bare, &layout.tap_work)?;
        let formula_path = layout.tap_work.join(self.formula_file());
        std::fs::create_dir_all(formula_path.parent().unwrap())?;
        std::fs::write(&formula_path, formula)?;
        tap::commit_all(&layout.tap_work, &format!("{} {}", self.build.package_name, ctx.job.job_variables.version))?;
        tap::push(&layout.tap_work)?;

        let brew = brew::Brew::new(ctx.logging.clone(), layout.log.clone());
        self.free_tap(&brew, &layout)?;
        let developer_mode = brew.developer_mode()?;

        let result = self.brew_build(&brew, &layout);

        let _ = brew.run(&["uninstall", "--ignore-dependencies", &self.formula_ref()]);
        let _ = brew.run(&["untap", &self.tap_name()]);
        if !developer_mode {
            let _ = brew.run(&["developer", "off"]);
        }
        result?;

        let metas = formula::parse_brew_bottle_json(&std::fs::read_to_string(bottle_json(&layout.output)?)?)?;
        let [meta] = metas.as_slice() else {
            anyhow::bail!("expected exactly one bottle, brew produced {}", metas.len());
        };
        std::fs::write(layout.output.join(META_FILE), serde_json::to_string_pretty(meta)?)?;
        Ok(())
    }

    fn free_tap(&self, brew: &brew::Brew, layout: &Layout) -> Result<()> {
        match brew.tap_remote(&self.tap_name())? {
            None => Ok(()),
            Some(remote) if Path::new(&remote) == layout.tap_bare => {
                let _ = brew.run(&["uninstall", "--ignore-dependencies", &self.formula_ref()]);
                brew.run(&["untap", &self.tap_name()])
            }
            Some(remote) => anyhow::bail!(
                "{} is already tapped from {}; run `brew uninstall {}; brew untap {}` before building",
                self.tap_name(),
                remote,
                self.formula_ref(),
                self.tap_name()
            ),
        }
    }

    fn brew_build(&self, brew: &brew::Brew, layout: &Layout) -> Result<()> {
        brew.run(&["tap", &self.tap_name(), &layout.tap_bare.to_string_lossy()])?;
        brew.run(&["install", "--build-bottle", &self.formula_ref()])?;
        let root_url = format!("--root-url=file://{}", layout.output.display());
        brew.run_in(&layout.output, &["bottle", "--json", "--no-rebuild", &root_url, &self.formula_ref()])
    }

    fn run_publish(&self, ctx: &TargetContext, repository: &Repository, custom_install_page: Option<PathBuf>) -> Result<()> {
        let layout = self.layout(ctx);
        let meta_path = layout.output.join(META_FILE);
        let meta: BottleMeta = serde_json::from_str(&std::fs::read_to_string(&meta_path).with_context(|| format!("no macos build output at {}, build it first", meta_path.display()))?)?;
        let version = ctx.job.job_variables.version.clone();
        let source_name = format!("{}.tar.gz", self.source_folder_name(ctx));
        let source_path = layout.output.join(&source_name);
        let source_sha = sha256_file(&source_path)?;

        let root = Store::new(repository);
        let store = root.scoped(ID);
        let bottle_key = format!("bottles/{}", meta.filename);

        store.put_file(&bottle_key, &layout.output.join(&meta.local_filename), Some("application/gzip"))?;
        store.put(&format!("meta/{version}/{}.json", meta.tag), serde_json::to_vec_pretty(&meta)?, Some("application/json"))?;

        let formula_base = self.render_formula(ctx, &format!("file://{}", source_path.display()), &source_sha)?;
        let root_url = store.public_url("bottles");
        let scratch = tempfile::tempdir_in(&layout.root)?;

        store.commit_anchored(TAP_ANCHOR, "text/plain", |_| {
            let attempt = tempfile::tempdir_in(scratch.path())?;
            let bare = attempt.path().join("tap.git");
            store.scoped("tap.git").download_dir(&bare)?;
            let before = tap::snapshot(&bare)?;
            if !bare.join("HEAD").exists() {
                tap::init_bare(&bare)?;
            }

            let work = attempt.path().join("work");
            tap::clone(&bare, &work)?;
            let metas = self.download_metas(&store, &version, &attempt.path().join("meta"))?;
            let formula = formula::insert_bottle(&formula_base, &source_sha, &formula::bottle_block(&root_url, &metas))?;
            let formula_path = work.join(self.formula_file());
            std::fs::create_dir_all(formula_path.parent().unwrap())?;
            std::fs::write(&formula_path, formula)?;
            if tap::commit_all(&work, &format!("{} {}", self.build.package_name, version))? {
                tap::push(&work)?;
            }
            tap::update_server_info(&bare)?;

            let derived = tap::changed_files(&before, &tap::snapshot(&bare)?)
                .into_iter()
                .map(|rel| Ok((format!("tap.git/{rel}"), "application/octet-stream", std::fs::read(bare.join(&rel))?)))
                .collect::<Result<Vec<_>>>()?;
            Ok((std::fs::read(bare.join("info/refs"))?, derived))
        })?;

        let tap_url = store.public_url("tap.git");
        let entry = repo_files::Repository::from([
            ("distro_id".to_string(), ID.to_string()),
            ("distro_name".to_string(), NAME.to_string()),
            ("distro_family".to_string(), FAMILY.to_string()),
            ("install_steps".to_string(), format!("brew tap {} {}\nbrew install {}", self.tap_name(), tap_url, self.formula_ref())),
            ("download_url".to_string(), store.public_url(&bottle_key)),
            ("package_type".to_string(), PACKAGE_TYPE.to_string()),
            ("install_as_root".to_string(), "false".to_string()),
            ("timestamp".to_string(), chrono::Utc::now().to_rfc3339()),
        ]);
        let custom_template = custom_install_page.as_deref().map(std::fs::read_to_string).transpose()?;
        let badge = index::publish_entry(repository, entry, custom_template, None)?;

        if let Err(e) = root.purge_cdn() {
            Logger::new().warn(format!("cannot purge cache: {:#}", e));
        }
        Logger::new().info(format!("tap: {}", colorize(Color::Green, &tap_url)));
        if !badge.page_url.is_empty() {
            Logger::new().info(format!("install page: {}", colorize(Color::Green, badge.page_url)));
        }
        if !badge.badge_md.is_empty() {
            Logger::new().info(format!("badge markdown: {}", colorize(Color::Yellow, badge.badge_md)));
        }
        Ok(())
    }

    fn download_metas(&self, store: &Store, version: &str, to: &Path) -> Result<Vec<BottleMeta>> {
        store.scoped(&format!("meta/{version}")).download_dir(to)?;
        let mut metas = vec![];
        for entry in std::fs::read_dir(to)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                metas.push(serde_json::from_str(&std::fs::read_to_string(&path)?).with_context(|| format!("cannot parse {}", path.display()))?);
            }
        }
        Ok(metas)
    }

    fn report(&self, stage: &str, ctx: &TargetContext, result: Result<()>) -> Result<bool> {
        let log = self.layout(ctx).log;
        let result = result.inspect_err(|e| Logger::new().error(format!("failed {} for {} ({:#}), log: {}", stage, ID, e, colorize(Color::Red, log.display()))));
        fail_fast_or_continue(result, ctx.fail_fast)
    }

    fn timed(&self, stage: &str, f: impl FnOnce() -> Result<()>) -> Result<()> {
        Logger::new().info(format!("starting {} for {}", stage, ID));
        let started_at = Instant::now();
        f()?;
        Logger::new().info(format!("successfully finished {} for {} in {:.1}s", stage, ID, started_at.elapsed().as_secs_f32()));
        Ok(())
    }
}

fn bottle_json(output: &Path) -> Result<PathBuf> {
    std::fs::read_dir(output)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.to_string_lossy().ends_with(".bottle.json"))
        .with_context(|| format!("brew bottle produced no json in {}", output.display()))
}

impl Target for MacTarget {
    fn build(&mut self, ctx: &TargetContext) -> Result<bool> {
        let result = self.timed("build", || self.run_build(ctx));
        self.report("build", ctx, result)
    }

    fn publish(&mut self, ctx: &TargetContext, repository: &Repository, custom_install_page: Option<PathBuf>) -> Result<bool> {
        let result = self.timed("repository publish", || self.run_publish(ctx, repository, custom_install_page));
        self.report("repository publish", ctx, result)
    }

    fn release(&mut self, ctx: &TargetContext, repository: &Repository, custom_install_page: Option<PathBuf>) -> Result<bool> {
        if !self.build(ctx)? {
            return Ok(false);
        }
        self.publish(ctx, repository, custom_install_page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LoggingArgs;
    use crate::config::HomebrewConfig;
    use crate::job_variables::JobVariables;
    use crate::release::JobSetup;

    fn build_config() -> Build {
        Build {
            distro: ID.to_string(),
            package_name: "hello".to_string(),
            maintainer: "m".to_string(),
            homepage: "https://example.com".to_string(),
            description: "Hello".to_string(),
            build_dependencies: vec!["cmake".to_string()],
            runtime_dependencies: vec![],
            before_build_script: None,
            rpm: None,
            deb: None,
            pacman: None,
            homebrew: Some(HomebrewConfig {
                formula_template: "formula.rb.liquid".to_string(),
            }),
            rest: HashMap::new(),
        }
    }

    fn job(dir: &Path) -> JobSetup {
        JobSetup {
            job_variables: JobVariables::new("1.2.3".to_string()),
            build_dir: dir.join("build"),
            source_dir: dir.join("src"),
            image_cache: None,
            ignore_source_files: vec![],
        }
    }

    fn logging() -> LoggingArgs {
        LoggingArgs {
            container_output: "null".to_string(),
            disable_container_echo: false,
            fail_log_lines: 0,
        }
    }

    #[test]
    fn renders_formula_with_target_vars() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/formula.rb.liquid"),
            "class {{ formula_class }} < Formula\n  url \"{{ url }}\"\n  sha256 \"{{ sha256 }}\"\n  version \"{{ version }}\"\n{% for d in build_dependencies %}  depends_on \"{{ d }}\" => :build\n{% endfor %}end\n",
        )
        .unwrap();
        let job = job(dir.path());
        let logging = logging();
        let ctx = TargetContext {
            job: &job,
            logging: &logging,
            fail_fast: true,
        };

        let formula = MacTarget::new(build_config()).render_formula(&ctx, "file:///build/hello-1.2.3.tar.gz", "abc").unwrap();
        assert_eq!(
            formula,
            "class Hello < Formula\n  url \"file:///build/hello-1.2.3.tar.gz\"\n  sha256 \"abc\"\n  version \"1.2.3\"\n  depends_on \"cmake\" => :build\nend\n"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn build_refuses_non_macos_host() {
        let dir = tempfile::tempdir().unwrap();
        let job = job(dir.path());
        let logging = logging();
        let ctx = TargetContext {
            job: &job,
            logging: &logging,
            fail_fast: true,
        };

        let err = MacTarget::new(build_config()).build(&ctx).unwrap_err();
        assert!(err.to_string().contains("only be built on a macOS host"), "{err}");
    }

    #[test]
    fn publish_without_build_output_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let job = job(dir.path());
        let logging = logging();
        let ctx = TargetContext {
            job: &job,
            logging: &logging,
            fail_fast: true,
        };

        let err = MacTarget::new(build_config()).run_publish(&ctx, &localfs_repository(&dir.path().join("repo")), None).unwrap_err();
        assert!(err.to_string().contains("build it first"), "{err}");
    }

    fn localfs_repository(path: &Path) -> Repository {
        Repository {
            name: "Local".to_string(),
            provider: crate::config::RepositoryProvider::LocalFs,
            localfs: Some(crate::config::LocalFsConfig {
                path: path.to_string_lossy().into_owned(),
            }),
            s3: None,
            gpg_private_key_base64: String::new(),
            package_name: "hello".to_string(),
            retain_packages: 0,
            rest: HashMap::new(),
        }
    }

    #[test]
    fn publish_writes_tap_bottles_and_index_to_localfs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/formula.rb.liquid"),
            "class {{ formula_class }} < Formula\n  url \"{{ url }}\"\n  sha256 \"{{ sha256 }}\"\nend\n",
        )
        .unwrap();
        let job = job(dir.path());
        let logging = logging();
        let ctx = TargetContext {
            job: &job,
            logging: &logging,
            fail_fast: true,
        };
        let target = MacTarget::new(build_config());

        let layout = target.layout(&ctx);
        std::fs::create_dir_all(&layout.output).unwrap();
        std::fs::write(layout.output.join("hello-1.2.3.tar.gz"), b"source").unwrap();
        std::fs::write(layout.output.join("hello--1.2.3.arm64_sequoia.bottle.tar.gz"), b"bottle").unwrap();
        let meta = BottleMeta {
            tag: "arm64_sequoia".to_string(),
            filename: "hello-1.2.3.arm64_sequoia.bottle.tar.gz".to_string(),
            local_filename: "hello--1.2.3.arm64_sequoia.bottle.tar.gz".to_string(),
            sha256: "bottlesha".to_string(),
            cellar: "any".to_string(),
            rebuild: 0,
        };
        std::fs::write(layout.output.join(META_FILE), serde_json::to_string(&meta).unwrap()).unwrap();

        let repo_dir = dir.path().join("repo");
        let repository = localfs_repository(&repo_dir);
        target.run_publish(&ctx, &repository, None).unwrap();

        assert_eq!(std::fs::read(repo_dir.join("macos/bottles/hello-1.2.3.arm64_sequoia.bottle.tar.gz")).unwrap(), b"bottle");
        assert!(!repo_dir.join("macos/src").exists(), "sources must not be published");
        assert!(repo_dir.join("macos/meta/1.2.3/arm64_sequoia.json").exists());
        assert!(repo_dir.join("macos/tap.git/info/refs").exists());

        let clone = dir.path().join("consumer");
        tap::clone(&repo_dir.join("macos/tap.git"), &clone).unwrap();
        let formula = std::fs::read_to_string(clone.join("Formula/hello.rb")).unwrap();
        assert!(formula.contains(&format!("url \"file://{}\"", layout.output.join("hello-1.2.3.tar.gz").display())), "{formula}");
        assert!(formula.contains(&format!("root_url \"{}\"", repo_dir.join("macos/bottles").display())), "{formula}");
        assert!(formula.contains("sha256 cellar: :any, arm64_sequoia: \"bottlesha\""), "{formula}");

        let json = repo_files::json::parse(&std::fs::read_to_string(repo_dir.join(index::INSTALL_JSON_NAME)).unwrap());
        let entry = json.iter().find(|e| e["distro_id"] == ID).unwrap();
        assert_eq!(entry["package_type"], PACKAGE_TYPE);
        assert!(entry["install_steps"].starts_with(&format!("brew tap omnipackage/hello {}", repo_dir.join("macos/tap.git").display())));

        target.run_publish(&ctx, &repository, None).unwrap();
        let log = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_dir.join("macos/tap.git"))
            .args(["rev-list", "--count", "main"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "1", "republishing identical bits must not add a commit");
    }
}
