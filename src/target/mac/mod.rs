use super::source_archive::{self, sha256_file};
use super::{Target, TargetContext};
use crate::config::{Build, Repository};
use crate::logger::{Color, Logger, colorize};
use crate::publish::store::Store;
use crate::publish::{index, repo_files};
use crate::release::fail_fast_or_continue;
use crate::shell::Command;
use crate::template::{Template, Var};
use anyhow::{Context, Result};
use cask::CaskArtifact;
use formula::BottleMeta;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

mod brew;
mod cask;
mod formula;
mod retention;
mod tap;

pub const ID: &str = "macos";
const NAME: &str = "macOS (Homebrew)";
const FAMILY: &str = "macOS";
const PACKAGE_TYPE: &str = "homebrew";
const TAP_USER: &str = "omnipackage";
const META_FILE: &str = "bottle-meta.json";
const CASK_META_FILE: &str = "cask-meta.json";
const TAP_ANCHOR: &str = "tap.git/info/refs";

enum Mode {
    Formula { template: String },
    Cask { template: String, build_script: String },
}

pub struct MacTarget {
    build: Build,
    mode: Mode,
}

struct Layout {
    root: PathBuf,
    output: PathBuf,
    stage: PathBuf,
    tap_work: PathBuf,
    tap_bare: PathBuf,
    log: PathBuf,
}

struct Published {
    install: Vec<String>,
    download_url: String,
}

impl MacTarget {
    pub fn new(build: Build) -> Result<Self> {
        let config = build.homebrew.clone().context("macos builds need a `homebrew` section")?;
        let mode = match (config.formula_template, config.cask_template, config.build_script) {
            (Some(template), None, None) => Mode::Formula { template },
            (None, Some(template), Some(build_script)) => Mode::Cask { template, build_script },
            (None, Some(_), None) => anyhow::bail!("homebrew.cask_template needs homebrew.build_script to build the app"),
            (Some(_), None, Some(_)) => anyhow::bail!("homebrew.build_script is for casks only: a formula builds in its install block"),
            _ => anyhow::bail!("homebrew needs exactly one of formula_template or cask_template"),
        };
        Ok(Self { build, mode })
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

    fn cask_file(&self) -> String {
        format!("Casks/{}.rb", self.build.package_name)
    }

    fn source_folder_name(&self, ctx: &TargetContext) -> String {
        format!("{}-{}", self.build.package_name, ctx.job.job_variables.version)
    }

    fn render(&self, ctx: &TargetContext, template: &str, extra: &[(&str, &str)]) -> Result<String> {
        let mut vars: HashMap<String, Var> = ctx.job.job_variables.to_template_vars();
        vars.extend(self.build.to_template_vars());
        vars.extend(extra.iter().map(|(k, v)| (k.to_string(), (*v).into())));
        Template::from_file(ctx.job.source_dir.join(template))?.render(vars)
    }

    fn render_formula(&self, ctx: &TargetContext, template: &str, url: &str, sha256: &str) -> Result<String> {
        let class = formula::formula_class(&self.build.package_name);
        self.render(
            ctx,
            template,
            &[("formula_class", &class), ("source_folder_name", &self.source_folder_name(ctx)), ("url", url), ("sha256", sha256)],
        )
    }

    fn run_build(&self, ctx: &TargetContext) -> Result<()> {
        if self.build.before_build_script.is_some() {
            Logger::new().warn("before_build_script is ignored for macos");
        }
        let layout = self.layout(ctx);
        if layout.root.exists() {
            std::fs::remove_dir_all(&layout.root).with_context(|| format!("cannot clean build dir {}", layout.root.display()))?;
        }
        std::fs::create_dir_all(&layout.output)?;
        match &self.mode {
            Mode::Formula { template } => self.build_formula(ctx, &layout, template),
            Mode::Cask { build_script, .. } => self.build_cask(ctx, &layout, build_script),
        }
    }

    fn build_cask(&self, ctx: &TargetContext, layout: &Layout, build_script: &str) -> Result<()> {
        let src = layout.stage.join(self.source_folder_name(ctx));
        source_archive::stage(&ctx.job.source_dir, &ctx.job.ignore_source_files, &src)?;
        let output = std::path::absolute(&layout.output)?;
        let secrets = &ctx.job.job_variables.secrets;
        secrets
            .iter()
            .fold(Command::new("sh"), |c, (k, v)| c.with_env(k, v))
            .args(["-c", build_script])
            .current_dir(&src)
            .with_env("OMNIPACKAGE_OUTPUT_DIR", output.to_string_lossy())
            .with_env("OMNIPACKAGE_VERSION", &ctx.job.job_variables.version)
            .stream_output_to(ctx.logging.container_logger().with_secrets(secrets.values().cloned().collect()))
            .log_to(&layout.log)
            .run()
            .context("build_script failed")?;

        let artifact = cask::find_artifact(&output, CASK_META_FILE)?;
        let meta = CaskArtifact {
            filename: artifact.file_name().unwrap().to_string_lossy().into_owned(),
            sha256: sha256_file(&artifact)?,
        };
        std::fs::write(output.join(CASK_META_FILE), serde_json::to_string_pretty(&meta)?)?;
        Ok(())
    }

    fn build_formula(&self, ctx: &TargetContext, layout: &Layout, template: &str) -> Result<()> {
        if std::env::consts::OS != "macos" {
            anyhow::bail!("{} packages can only be built on a macOS host (this is {})", ID, std::env::consts::OS);
        }
        if !ctx.job.job_variables.secrets.is_empty() {
            Logger::new().warn("secrets are not passed to Homebrew builds");
        }

        let archive = source_archive::create(&ctx.job.source_dir, &ctx.job.ignore_source_files, &self.source_folder_name(ctx), &layout.stage, &layout.output)?;
        let formula = self.render_formula(ctx, template, &format!("file://{}", archive.path.display()), &archive.sha256)?;

        tap::init_bare(&layout.tap_bare)?;
        tap::clone(&layout.tap_bare, &layout.tap_work)?;
        let formula_path = layout.tap_work.join(self.formula_file());
        std::fs::create_dir_all(formula_path.parent().unwrap())?;
        std::fs::write(&formula_path, formula)?;
        tap::commit_all(&layout.tap_work, &format!("{} {}", self.build.package_name, ctx.job.job_variables.version))?;
        tap::push(&layout.tap_work)?;

        let brew = brew::Brew::new(ctx.logging.clone(), layout.log.clone());
        self.free_tap(&brew, layout)?;
        let developer_mode = brew.developer_mode()?;

        let result = self.brew_build(&brew, layout);

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
        let root = Store::new(repository);
        let store = root.scoped(ID);
        let published = match &self.mode {
            Mode::Formula { template } => self.publish_formula(ctx, &layout, &store, template)?,
            Mode::Cask { template, .. } => self.publish_cask(ctx, &layout, &store, template)?,
        };
        let dirs: &[&str] = match self.mode {
            Mode::Formula { .. } => &["bottles", "meta"],
            Mode::Cask { .. } => &["casks"],
        };
        match retention::prune(&store, dirs, &ctx.job.job_variables.version, repository.retain_packages as usize) {
            Ok(p) => Logger::new().info(format!("retention for {}: kept {} versions, removed {}", ID, p.kept, p.removed)),
            Err(e) => Logger::new().warn(format!("cannot remove old versions: {:#}", e)),
        }

        let tap_url = store.public_url("tap.git");
        let install_steps = std::iter::once(format!("brew tap {} {}", self.tap_name(), tap_url))
            .chain(published.install)
            .collect::<Vec<_>>()
            .join("\n");
        let entry = repo_files::Repository::from([
            ("distro_id".to_string(), ID.to_string()),
            ("distro_name".to_string(), NAME.to_string()),
            ("distro_family".to_string(), FAMILY.to_string()),
            ("install_steps".to_string(), install_steps),
            ("download_url".to_string(), published.download_url),
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

    fn publish_formula(&self, ctx: &TargetContext, layout: &Layout, store: &Store, template: &str) -> Result<Published> {
        let meta_path = layout.output.join(META_FILE);
        let meta: BottleMeta = serde_json::from_str(&std::fs::read_to_string(&meta_path).with_context(|| format!("no macos build output at {}, build it first", meta_path.display()))?)?;
        let version = ctx.job.job_variables.version.clone();
        let source_path = layout.output.join(format!("{}.tar.gz", self.source_folder_name(ctx)));
        let source_sha = sha256_file(&source_path)?;
        let bottle_key = format!("bottles/{version}/{}", meta.filename);

        store.put_file(&bottle_key, &layout.output.join(&meta.local_filename), Some("application/gzip"))?;
        store.put(&format!("meta/{version}/{}.json", meta.tag), serde_json::to_vec_pretty(&meta)?, Some("application/json"))?;

        let formula_base = self.render_formula(ctx, template, &format!("file://{}", source_path.display()), &source_sha)?;
        let root_url = download_url(store, &format!("bottles/{version}"));
        self.commit_tap(store, layout, &self.formula_file(), &version, |attempt| {
            let metas = self.download_metas(store, &version, &attempt.join("meta"))?;
            formula::insert_bottle(&formula_base, &source_sha, &formula::bottle_block(&root_url, &metas))
        })?;
        Ok(Published {
            install: vec![format!("brew install {}", self.formula_ref())],
            download_url: download_url(store, &bottle_key),
        })
    }

    fn publish_cask(&self, ctx: &TargetContext, layout: &Layout, store: &Store, template: &str) -> Result<Published> {
        let meta_path = layout.output.join(CASK_META_FILE);
        let meta: CaskArtifact = serde_json::from_str(&std::fs::read_to_string(&meta_path).with_context(|| format!("no macos build output at {}, build it first", meta_path.display()))?)?;
        let version = ctx.job.job_variables.version.clone();
        let key = format!("casks/{version}/{}", meta.filename);

        store.put_file(&key, &layout.output.join(&meta.filename), Some(cask::content_type(&meta.filename)))?;
        let url = download_url(store, &key);
        let cask = self.render(ctx, template, &[("cask_token", &self.build.package_name), ("url", &url), ("sha256", &meta.sha256)])?;
        self.commit_tap(store, layout, &self.cask_file(), &version, |_| Ok(cask.clone()))?;

        let mut install = vec![format!("brew install --cask {}", self.formula_ref())];
        install.extend(cask::quarantine_steps(&cask));
        Ok(Published { install, download_url: url })
    }

    fn commit_tap(&self, store: &Store, layout: &Layout, file: &str, version: &str, render: impl Fn(&Path) -> Result<String>) -> Result<()> {
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
            let content = render(attempt.path())?;
            let path = work.join(file);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(&path, content)?;
            if tap::commit_all(&work, &format!("{} {}", self.build.package_name, version))? {
                tap::push(&work)?;
            }
            tap::update_server_info(&bare)?;

            let derived = tap::changed_files(&before, &tap::snapshot(&bare)?)
                .into_iter()
                .map(|rel| Ok((format!("tap.git/{rel}"), "application/octet-stream", std::fs::read(bare.join(&rel))?)))
                .collect::<Result<Vec<_>>>()?;
            Ok((std::fs::read(bare.join("info/refs"))?, derived))
        })
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

fn download_url(store: &Store, key: &str) -> String {
    match store {
        Store::LocalFs { .. } => format!("file://{}", store.public_url(key)),
        Store::S3 { .. } => store.public_url(key),
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

    fn homebrew(formula: Option<&str>, cask: Option<&str>, script: Option<&str>) -> HomebrewConfig {
        HomebrewConfig {
            formula_template: formula.map(str::to_string),
            cask_template: cask.map(str::to_string),
            build_script: script.map(str::to_string),
        }
    }

    fn build_config_with(homebrew: HomebrewConfig) -> Build {
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
            homebrew: Some(homebrew),
            rest: HashMap::new(),
        }
    }

    fn build_config() -> Build {
        build_config_with(homebrew(Some("formula.rb.liquid"), None, None))
    }

    fn cask_config() -> Build {
        build_config_with(homebrew(None, Some("cask.rb.liquid"), Some("sh build.sh")))
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

    fn ctx<'a>(job: &'a JobSetup, logging: &'a LoggingArgs) -> TargetContext<'a> {
        TargetContext { job, logging, fail_fast: true }
    }

    #[test]
    fn mode_comes_from_homebrew_config() {
        assert!(matches!(MacTarget::new(build_config()).unwrap().mode, Mode::Formula { .. }));
        assert!(matches!(MacTarget::new(cask_config()).unwrap().mode, Mode::Cask { .. }));
        for (config, expected) in [
            (homebrew(None, Some("c"), None), "needs homebrew.build_script"),
            (homebrew(Some("f"), None, Some("s")), "for casks only"),
            (homebrew(Some("f"), Some("c"), None), "exactly one"),
            (homebrew(None, None, None), "exactly one"),
        ] {
            let err = MacTarget::new(build_config_with(config)).err().unwrap().to_string();
            assert!(err.contains(expected), "{err}");
        }
        let mut no_section = build_config();
        no_section.homebrew = None;
        assert!(MacTarget::new(no_section).is_err());
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

        let formula = MacTarget::new(build_config())
            .unwrap()
            .render_formula(&ctx(&job, &logging), "formula.rb.liquid", "file:///build/hello-1.2.3.tar.gz", "abc")
            .unwrap();
        assert_eq!(
            formula,
            "class Hello < Formula\n  url \"file:///build/hello-1.2.3.tar.gz\"\n  sha256 \"abc\"\n  version \"1.2.3\"\n  depends_on \"cmake\" => :build\nend\n"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn formula_build_refuses_non_macos_host() {
        let dir = tempfile::tempdir().unwrap();
        let job = job(dir.path());
        let logging = logging();

        let err = MacTarget::new(build_config()).unwrap().build(&ctx(&job, &logging)).unwrap_err();
        assert!(err.to_string().contains("only be built on a macOS host"), "{err}");
    }

    #[test]
    fn publish_without_build_output_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let job = job(dir.path());
        let logging = logging();
        for config in [build_config(), cask_config()] {
            let err = MacTarget::new(config)
                .unwrap()
                .run_publish(&ctx(&job, &logging), &localfs_repository(&dir.path().join("repo")), None)
                .unwrap_err();
            assert!(err.to_string().contains("build it first"), "{err}");
        }
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

    fn rev_count(bare: &Path) -> String {
        let out = std::process::Command::new("git").arg("-C").arg(bare).args(["rev-list", "--count", "main"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn install_entry(repo_dir: &Path) -> repo_files::Repository {
        let json = repo_files::json::parse(&std::fs::read_to_string(repo_dir.join(index::INSTALL_JSON_NAME)).unwrap());
        json.into_iter().find(|e| e["distro_id"] == ID).unwrap()
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
        let ctx = ctx(&job, &logging);
        let target = MacTarget::new(build_config()).unwrap();

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

        assert_eq!(std::fs::read(repo_dir.join("macos/bottles/1.2.3/hello-1.2.3.arm64_sequoia.bottle.tar.gz")).unwrap(), b"bottle");
        assert!(!repo_dir.join("macos/src").exists(), "sources must not be published");
        assert!(repo_dir.join("macos/meta/1.2.3/arm64_sequoia.json").exists());
        assert!(repo_dir.join("macos/tap.git/info/refs").exists());

        let clone = dir.path().join("consumer");
        tap::clone(&repo_dir.join("macos/tap.git"), &clone).unwrap();
        let formula = std::fs::read_to_string(clone.join("Formula/hello.rb")).unwrap();
        assert!(formula.contains(&format!("url \"file://{}\"", layout.output.join("hello-1.2.3.tar.gz").display())), "{formula}");
        assert!(formula.contains(&format!("root_url \"file://{}\"", repo_dir.join("macos/bottles/1.2.3").display())), "{formula}");
        assert!(formula.contains("sha256 cellar: :any, arm64_sequoia: \"bottlesha\""), "{formula}");

        let entry = install_entry(&repo_dir);
        assert_eq!(entry["package_type"], PACKAGE_TYPE);
        assert_eq!(
            entry["install_steps"],
            format!("brew tap omnipackage/hello {}\nbrew install omnipackage/hello/hello", repo_dir.join("macos/tap.git").display())
        );

        target.run_publish(&ctx, &repository, None).unwrap();
        assert_eq!(rev_count(&repo_dir.join("macos/tap.git")), "1", "republishing identical bits must not add a commit");
    }

    #[test]
    fn cask_build_runs_script_in_staged_source_and_publishes_to_localfs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("build.sh"),
            "set -eu\n[ ! -e .env ]\nprintf 'app %s %s' \"$OMNIPACKAGE_VERSION\" \"$TOKEN\" > \"$OMNIPACKAGE_OUTPUT_DIR/Hello-$OMNIPACKAGE_VERSION.dmg\"\ntouch built-here\n",
        )
        .unwrap();
        std::fs::write(src.join(".env"), "SECRET=1").unwrap();
        std::fs::write(
            src.join("cask.rb.liquid"),
            "cask \"{{ cask_token }}\" do\n  version \"{{ version }}\"\n  sha256 \"{{ sha256 }}\"\n  url \"{{ url }}\"\n  desc \"{{ description }}\"\n  app \"Hello World.app\"\nend\n",
        )
        .unwrap();
        let mut job = job(dir.path());
        job.ignore_source_files = vec![".env".to_string()];
        job.job_variables = job.job_variables.with_secrets(HashMap::from([("TOKEN".to_string(), "s3cr3t".to_string())]));
        let logging = logging();
        let ctx = ctx(&job, &logging);
        let target = MacTarget::new(cask_config()).unwrap();

        assert!(target.run_build(&ctx).is_ok());
        let layout = target.layout(&ctx);
        assert!(!src.join("built-here").exists(), "build_script must run in a staged copy");
        assert!(layout.stage.join("hello-1.2.3/built-here").exists());
        let meta: CaskArtifact = serde_json::from_str(&std::fs::read_to_string(layout.output.join(CASK_META_FILE)).unwrap()).unwrap();
        assert_eq!(meta.filename, "Hello-1.2.3.dmg");
        assert_eq!(meta.sha256, sha256_file(&layout.output.join("Hello-1.2.3.dmg")).unwrap());

        let repo_dir = dir.path().join("repo");
        let repository = localfs_repository(&repo_dir);
        target.run_publish(&ctx, &repository, None).unwrap();

        let artifact = repo_dir.join("macos/casks/1.2.3/Hello-1.2.3.dmg");
        assert_eq!(std::fs::read_to_string(&artifact).unwrap(), "app 1.2.3 s3cr3t");
        assert!(!repo_dir.join("macos/src").exists(), "sources must not be published");

        let clone = dir.path().join("consumer");
        tap::clone(&repo_dir.join("macos/tap.git"), &clone).unwrap();
        let cask = std::fs::read_to_string(clone.join("Casks/hello.rb")).unwrap();
        assert_eq!(
            cask,
            format!(
                "cask \"hello\" do\n  version \"1.2.3\"\n  sha256 \"{}\"\n  url \"file://{}\"\n  desc \"Hello\"\n  app \"Hello World.app\"\nend\n",
                meta.sha256,
                artifact.display()
            )
        );
        assert!(!clone.join("Formula").exists());

        let entry = install_entry(&repo_dir);
        assert_eq!(
            entry["install_steps"],
            format!(
                "brew tap omnipackage/hello {}\nbrew install --cask omnipackage/hello/hello\nxattr -dr com.apple.quarantine \"/Applications/Hello World.app\"",
                repo_dir.join("macos/tap.git").display()
            )
        );
        assert_eq!(entry["download_url"], format!("file://{}", artifact.display()));
        assert_eq!(entry["install_as_root"], "false");

        target.run_publish(&ctx, &repository, None).unwrap();
        assert_eq!(rev_count(&repo_dir.join("macos/tap.git")), "1", "republishing identical bits must not add a commit");
    }

    #[test]
    fn publishing_a_new_version_removes_old_ones_beyond_retain_packages() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("build.sh"), "printf x > \"$OMNIPACKAGE_OUTPUT_DIR/app-$OMNIPACKAGE_VERSION.zip\"\n").unwrap();
        std::fs::write(src.join("cask.rb.liquid"), "cask \"{{ cask_token }}\" do\n  url \"{{ url }}\"\nend\n").unwrap();
        let logging = logging();
        let target = MacTarget::new(cask_config()).unwrap();
        let repo_dir = dir.path().join("repo");
        let mut repository = localfs_repository(&repo_dir);
        let release = |version: &str, repository: &Repository| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let mut job = job(dir.path());
            job.job_variables = JobVariables::new(version.to_string());
            let ctx = ctx(&job, &logging);
            target.run_build(&ctx).unwrap();
            target.run_publish(&ctx, repository, None).unwrap();
        };
        let versions = || {
            let mut v: Vec<String> = std::fs::read_dir(repo_dir.join("macos/casks"))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        };

        release("1.0", &repository);
        release("1.1", &repository);
        assert_eq!(versions(), vec!["1.1"]);

        repository.retain_packages = 1;
        release("1.2", &repository);
        release("1.3", &repository);
        assert_eq!(versions(), vec!["1.2", "1.3"]);

        release("1.3", &repository);
        assert_eq!(versions(), vec!["1.2", "1.3"], "republishing the current version keeps the previous one");
    }

    #[test]
    fn cask_build_fails_when_script_fails_or_leaves_no_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let job = job(dir.path());
        let logging = logging();
        let ctx = ctx(&job, &logging);

        std::fs::write(src.join("build.sh"), "exit 3\n").unwrap();
        let err = MacTarget::new(cask_config()).unwrap().run_build(&ctx).unwrap_err();
        assert!(format!("{err:#}").contains("build_script failed"), "{err:#}");

        std::fs::write(src.join("build.sh"), "true\n").unwrap();
        let err = MacTarget::new(cask_config()).unwrap().run_build(&ctx).unwrap_err();
        assert!(err.to_string().contains("produced nothing"), "{err}");
    }
}
