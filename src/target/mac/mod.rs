use super::source_archive::{self, sha256_file};
use super::{Target, TargetContext};
use crate::config::{Build, Repository};
use crate::logger::{Color, Logger, colorize};
use crate::package::container_rm_args;
use crate::publish::store::Store;
use crate::publish::{index, repo_files};
use crate::release::fail_fast_or_continue;
use crate::shell::Command;
use crate::template::{Template, Var};
use anyhow::{Context, Result};
use artifact::Artifact;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

mod artifact;
mod cask;
mod formula;
mod retention;
mod tap;

pub const ID: &str = "macos";
const NAME: &str = "macOS (Homebrew)";
const FAMILY: &str = "macOS";
const PACKAGE_TYPE: &str = "homebrew";
const TAP_USER: &str = "omnipackage";
const META_FILE: &str = "artifact.json";
const TAP_ANCHOR: &str = "tap.git/info/refs";

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Formula,
    Cask,
}

impl Kind {
    fn store_dir(self) -> &'static str {
        match self {
            Kind::Formula => "formulae",
            Kind::Cask => "casks",
        }
    }

    fn tap_dir(self) -> &'static str {
        match self {
            Kind::Formula => "Formula",
            Kind::Cask => "Casks",
        }
    }
}

pub struct MacTarget {
    build: Build,
    kind: Kind,
    template: String,
    build_script: String,
    image: Option<String>,
}

struct Layout {
    root: PathBuf,
    output: PathBuf,
    stage: PathBuf,
    log: PathBuf,
}

impl MacTarget {
    pub fn new(build: Build) -> Result<Self> {
        let config = build.homebrew.clone().context("macos builds need a `homebrew` section")?;
        let (kind, template) = match (config.formula_template, config.cask_template) {
            (Some(template), None) => (Kind::Formula, template),
            (None, Some(template)) => (Kind::Cask, template),
            _ => anyhow::bail!("homebrew needs exactly one of formula_template or cask_template"),
        };
        let build_script = config.build_script.context("homebrew.build_script is required: it builds the file the formula or cask installs")?;
        Ok(Self {
            build,
            kind,
            template,
            build_script,
            image: config.image,
        })
    }

    pub fn is_containerized(build: &Build) -> bool {
        build.homebrew.as_ref().is_some_and(|h| h.image.is_some())
    }

    fn layout(&self, ctx: &TargetContext) -> Layout {
        let root = ctx.job.build_dir.join(format!("{}-{}", self.build.package_name, ID));
        Layout {
            output: root.join("output"),
            stage: root.join("src"),
            log: root.join("homebrew.log"),
            root,
        }
    }

    fn tap_name(&self) -> String {
        format!("{TAP_USER}/{}", self.build.package_name)
    }

    fn tap_ref(&self) -> String {
        format!("{}/{}", self.tap_name(), self.build.package_name)
    }

    fn tap_file(&self) -> String {
        format!("{}/{}.rb", self.kind.tap_dir(), self.build.package_name)
    }

    fn source_folder_name(&self, ctx: &TargetContext) -> String {
        format!("{}-{}", self.build.package_name, ctx.job.job_variables.version)
    }

    fn render(&self, ctx: &TargetContext, extra: &[(&str, &str)]) -> Result<String> {
        let mut vars: HashMap<String, Var> = ctx.job.job_variables.to_template_vars();
        vars.extend(self.build.to_template_vars());
        vars.extend(extra.iter().map(|(k, v)| (k.to_string(), (*v).into())));
        Template::from_file(ctx.job.source_dir.join(&self.template))?.render(vars)
    }

    fn run_build(&self, ctx: &TargetContext) -> Result<()> {
        if self.build.before_build_script.is_some() {
            Logger::new().warn("before_build_script is ignored for macos");
        }
        let layout = self.layout(ctx);
        self.clean(&layout.root)?;
        std::fs::create_dir_all(&layout.output)?;

        let src = std::path::absolute(layout.stage.join(self.source_folder_name(ctx)))?;
        source_archive::stage(&ctx.job.source_dir, &ctx.job.ignore_source_files, &src)?;
        let output = std::path::absolute(&layout.output)?;
        let secrets = &ctx.job.job_variables.secrets;
        self.build_command(&src, &output, &ctx.job.job_variables.version, secrets)
            .stream_output_to(ctx.logging.container_logger().with_secrets(secrets.values().cloned().collect()))
            .log_to(&layout.log)
            .run()
            .context("build_script failed")?;

        let file = artifact::find(&output, META_FILE)?;
        let meta = Artifact {
            filename: file.file_name().unwrap().to_string_lossy().into_owned(),
            sha256: sha256_file(&file)?,
        };
        std::fs::write(output.join(META_FILE), serde_json::to_string_pretty(&meta)?)?;
        Ok(())
    }

    fn build_command(&self, src: &Path, output: &Path, version: &str, secrets: &HashMap<String, String>) -> Command {
        match &self.image {
            Some(image) => Command::container(container_args(image, &self.build_script, src, output, version, secrets)),
            None => secrets
                .iter()
                .fold(Command::new("sh"), |c, (k, v)| c.with_env(k, v))
                .args(["-c", &self.build_script])
                .current_dir(src)
                .with_env("OMNIPACKAGE_OUTPUT_DIR", output.to_string_lossy())
                .with_env("OMNIPACKAGE_VERSION", version),
        }
    }

    fn clean(&self, root: &Path) -> Result<()> {
        if !root.exists() || std::fs::remove_dir_all(root).is_ok() {
            return Ok(());
        }
        let image = self.image.as_deref().with_context(|| format!("cannot clean build dir {}", root.display()))?;
        Command::container(container_rm_args(root, image))
            .run()
            .with_context(|| format!("cannot clean build dir {}", root.display()))
    }

    fn run_publish(&self, ctx: &TargetContext, repository: &Repository, custom_install_page: Option<PathBuf>) -> Result<()> {
        let layout = self.layout(ctx);
        let meta_path = layout.output.join(META_FILE);
        let meta: Artifact = serde_json::from_str(&std::fs::read_to_string(&meta_path).with_context(|| format!("no macos build output at {}, build it first", meta_path.display()))?)?;
        let version = ctx.job.job_variables.version.clone();
        let root = Store::new(repository);
        let store = root.scoped(ID);

        let key = format!("{}/{version}/{}", self.kind.store_dir(), meta.filename);
        store.put_file(&key, &layout.output.join(&meta.filename), Some(artifact::content_type(&meta.filename)))?;
        let url = download_url(&store, &key);
        let content = self.render(
            ctx,
            &[
                ("formula_class", &formula::formula_class(&self.build.package_name)),
                ("cask_token", &self.build.package_name),
                ("url", &url),
                ("sha256", &meta.sha256),
            ],
        )?;
        self.commit_tap(&store, &layout, &content, &version)?;

        match retention::prune(&store, &[self.kind.store_dir()], &version, repository.retain_packages as usize) {
            Ok(p) => Logger::new().info(format!("retention for {}: kept {} versions, removed {}", ID, p.kept, p.removed)),
            Err(e) => Logger::new().warn(format!("cannot remove old versions: {:#}", e)),
        }

        let tap_url = store.public_url("tap.git");
        let install = match self.kind {
            Kind::Formula => vec![format!("brew install {}", self.tap_ref())],
            Kind::Cask => std::iter::once(format!("brew install --cask {}", self.tap_ref())).chain(cask::quarantine_steps(&content)).collect(),
        };
        let install_steps = std::iter::once(format!("brew tap {} {}", self.tap_name(), tap_url)).chain(install).collect::<Vec<_>>().join("\n");
        let entry = repo_files::Repository::from([
            ("distro_id".to_string(), ID.to_string()),
            ("distro_name".to_string(), NAME.to_string()),
            ("distro_family".to_string(), FAMILY.to_string()),
            ("install_steps".to_string(), install_steps),
            ("download_url".to_string(), url),
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

    fn commit_tap(&self, store: &Store, layout: &Layout, content: &str, version: &str) -> Result<()> {
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
            let path = work.join(self.tap_file());
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

fn container_args(image: &str, script: &str, src: &Path, output: &Path, version: &str, secrets: &HashMap<String, String>) -> Vec<String> {
    let mut args: Vec<String> = ["run", "--rm", "--entrypoint", "/bin/sh", "--workdir", "/src"].map(String::from).to_vec();
    args.extend(["--mount".to_string(), format!("type=bind,source={},target=/src", src.display())]);
    args.extend(["--mount".to_string(), format!("type=bind,source={},target=/output", output.display())]);
    let env = BTreeMap::from([("OMNIPACKAGE_OUTPUT_DIR", "/output"), ("OMNIPACKAGE_VERSION", version)]);
    let secrets: BTreeMap<&str, &str> = secrets.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    for (k, v) in env.into_iter().chain(secrets) {
        args.extend(["-e".to_string(), format!("{k}={v}")]);
    }
    args.extend([image.to_string(), "-c".to_string(), script.to_string()]);
    args
}

fn download_url(store: &Store, key: &str) -> String {
    match store {
        Store::LocalFs { .. } => format!("file://{}", store.public_url(key)),
        Store::S3 { .. } => store.public_url(key),
    }
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
            image: None,
        }
    }

    fn build_config_with(homebrew: HomebrewConfig) -> Build {
        Build {
            distro: ID.to_string(),
            package_name: "hello".to_string(),
            maintainer: "m".to_string(),
            homepage: "https://example.com".to_string(),
            description: "Hello".to_string(),
            build_dependencies: vec![],
            runtime_dependencies: vec![],
            before_build_script: None,
            rpm: None,
            deb: None,
            pacman: None,
            homebrew: Some(homebrew),
            rest: HashMap::new(),
        }
    }

    fn formula_config() -> Build {
        build_config_with(homebrew(Some("formula.rb.liquid"), None, Some("sh build.sh")))
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
    fn kind_comes_from_homebrew_config() {
        assert_eq!(MacTarget::new(formula_config()).unwrap().kind, Kind::Formula);
        assert_eq!(MacTarget::new(cask_config()).unwrap().kind, Kind::Cask);
        for (config, expected) in [
            (homebrew(None, Some("c"), None), "build_script is required"),
            (homebrew(Some("f"), None, None), "build_script is required"),
            (homebrew(Some("f"), Some("c"), Some("s")), "exactly one"),
            (homebrew(None, None, Some("s")), "exactly one"),
        ] {
            let err = MacTarget::new(build_config_with(config)).err().unwrap().to_string();
            assert!(err.contains(expected), "{err}");
        }
        let mut no_section = formula_config();
        no_section.homebrew = None;
        assert!(MacTarget::new(no_section).is_err());
    }

    #[test]
    fn container_runs_build_script_with_mounted_source_and_output() {
        let secrets = HashMap::from([("TOKEN".to_string(), "s3cr3t".to_string())]);
        let args = container_args("golang:1.25", "make dist", Path::new("/b/src/hello-1.2.3"), Path::new("/b/output"), "1.2.3", &secrets);
        assert_eq!(
            args,
            [
                "run",
                "--rm",
                "--entrypoint",
                "/bin/sh",
                "--workdir",
                "/src",
                "--mount",
                "type=bind,source=/b/src/hello-1.2.3,target=/src",
                "--mount",
                "type=bind,source=/b/output,target=/output",
                "-e",
                "OMNIPACKAGE_OUTPUT_DIR=/output",
                "-e",
                "OMNIPACKAGE_VERSION=1.2.3",
                "-e",
                "TOKEN=s3cr3t",
                "golang:1.25",
                "-c",
                "make dist",
            ]
        );
    }

    #[test]
    fn containerized_only_with_image() {
        let mut build = formula_config();
        assert!(!MacTarget::is_containerized(&build));
        build.homebrew.as_mut().unwrap().image = Some("golang:1.25".to_string());
        assert!(MacTarget::is_containerized(&build));
    }

    #[test]
    fn publish_without_build_output_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let job = job(dir.path());
        let logging = logging();
        for config in [formula_config(), cask_config()] {
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
    fn formula_build_publishes_prebuilt_archive_tap_and_index_to_localfs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("build.sh"),
            "set -eu\nprintf bin > hello\ntar -czf \"$OMNIPACKAGE_OUTPUT_DIR/hello-$OMNIPACKAGE_VERSION-macos.tar.gz\" hello\n",
        )
        .unwrap();
        std::fs::write(
            src.join("formula.rb.liquid"),
            "class {{ formula_class }} < Formula\n  url \"{{ url }}\"\n  sha256 \"{{ sha256 }}\"\n  version \"{{ version }}\"\n\n  def install\n    bin.install \"hello\"\n  end\nend\n",
        )
        .unwrap();
        let job = job(dir.path());
        let logging = logging();
        let ctx = ctx(&job, &logging);
        let target = MacTarget::new(formula_config()).unwrap();

        target.run_build(&ctx).unwrap();
        let archive = target.layout(&ctx).output.join("hello-1.2.3-macos.tar.gz");
        let sha = sha256_file(&archive).unwrap();

        let repo_dir = dir.path().join("repo");
        let repository = localfs_repository(&repo_dir);
        target.run_publish(&ctx, &repository, None).unwrap();

        let published = repo_dir.join("macos/formulae/1.2.3/hello-1.2.3-macos.tar.gz");
        assert_eq!(std::fs::read(&published).unwrap(), std::fs::read(&archive).unwrap());
        assert!(!repo_dir.join("macos/src").exists(), "sources must not be published");

        let clone = dir.path().join("consumer");
        tap::clone(&repo_dir.join("macos/tap.git"), &clone).unwrap();
        let formula = std::fs::read_to_string(clone.join("Formula/hello.rb")).unwrap();
        assert_eq!(
            formula,
            format!(
                "class Hello < Formula\n  url \"file://{}\"\n  sha256 \"{sha}\"\n  version \"1.2.3\"\n\n  def install\n    bin.install \"hello\"\n  end\nend\n",
                published.display()
            )
        );
        assert!(!clone.join("Casks").exists());

        let entry = install_entry(&repo_dir);
        assert_eq!(entry["package_type"], PACKAGE_TYPE);
        assert_eq!(entry["download_url"], format!("file://{}", published.display()));
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
        let meta: Artifact = serde_json::from_str(&std::fs::read_to_string(layout.output.join(META_FILE)).unwrap()).unwrap();
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
    fn build_fails_when_script_fails_or_leaves_no_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let job = job(dir.path());
        let logging = logging();
        let ctx = ctx(&job, &logging);

        std::fs::write(src.join("build.sh"), "exit 3\n").unwrap();
        let err = MacTarget::new(formula_config()).unwrap().run_build(&ctx).unwrap_err();
        assert!(format!("{err:#}").contains("build_script failed"), "{err:#}");

        std::fs::write(src.join("build.sh"), "true\n").unwrap();
        let err = MacTarget::new(formula_config()).unwrap().run_build(&ctx).unwrap_err();
        assert!(err.to_string().contains("produced nothing"), "{err}");
    }
}
