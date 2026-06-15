use crate::config::{Build, ImageCache, Repository};
use crate::distros::Distro;
use crate::gpg::Key;
use crate::job_variables::JobVariables;
use crate::package::{Package, SetupStage};
use crate::template::{Template, Var};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

// Host architecture in AppImage naming (x86_64, aarch64, armhf, i686), detected at runtime.
// The build runs in a native container, so the host arch equals the produced artefact's arch.
pub fn appimage_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "arm" => "armhf",
        "x86" => "i686",
        other => other,
    }
}

#[derive(Debug, Clone)]
pub struct Appimage {
    pub distro: Distro,
    pub source_dir: PathBuf,
    pub job_variables: JobVariables,
    pub distro_build_dir: PathBuf,
    pub image_cache: Option<ImageCache>,
    pub ignore_source_files: Vec<String>,

    mounts: HashMap<String, String>,
    commands: Vec<String>,
    build_output_dir: PathBuf,
    setup_stages: Vec<SetupStage>,
    gpgkey: Option<Key>,
    // Carried from setup_build (where the Build config lives) to setup_repository, which
    // only receives the Repository config. zsync needs the public URL to embed update
    // info, so it is generated in the repository stage — but the opt-in lives on the build.
    zsync: bool,
}

impl Appimage {
    pub fn new(distro: Distro, source_dir: PathBuf, job_variables: JobVariables, distro_build_dir: PathBuf, image_cache: Option<ImageCache>, ignore_source_files: Vec<String>) -> Self {
        Self {
            distro,
            source_dir,
            job_variables,
            image_cache,
            ignore_source_files,
            distro_build_dir: distro_build_dir.clone(),
            mounts: HashMap::new(),
            commands: Vec::new(),
            build_output_dir: distro_build_dir.clone(),
            setup_stages: Vec::new(),
            gpgkey: None,
            zsync: false,
        }
    }

    // Working dir for the build recipe + assembled AppDir. Bind-mounted to /work.
    fn work_path(&self) -> PathBuf {
        self.distro_build_dir().join("work")
    }

    // Where the recipe drops the built AppImage. Bind-mounted to /output.
    fn output_path(&self) -> PathBuf {
        self.distro_build_dir().join("output")
    }

    // Stable, version-less filename: zsync needs a constant URL across releases; the
    // version lives inside the AppImage. Arch is detected at runtime, not hardcoded.
    fn appimage_name(package_name: &str) -> String {
        format!("{package_name}-{}.AppImage", appimage_arch())
    }
}

impl Package for Appimage {
    fn setup_build(&mut self, config: Build) -> Result<(), anyhow::Error> {
        self.prepare_build_dir()?;
        let appimage = config.appimage.clone().ok_or_else(|| anyhow::anyhow!("appimage config is missing"))?;
        self.zsync = appimage.zsync;

        let work_path = self.work_path();
        let output_path = self.output_path();
        std::fs::create_dir_all(&work_path).with_context(|| format!("cannot create directory {}", work_path.display()))?;
        std::fs::create_dir_all(&output_path).with_context(|| format!("cannot create directory {}", output_path.display()))?;

        let mut template_vars: HashMap<String, Var> = self.job_variables.to_template_vars();
        template_vars.extend(config.to_template_vars());
        let template = Template::from_file(self.source_dir.join(&appimage.recipe_template))?;
        template.render_to_file(template_vars, work_path.join("build-appimage.sh"))?;

        self.mounts.insert(self.source_dir.to_string_lossy().to_string(), "/source".to_string());
        self.mounts.insert(work_path.to_string_lossy().to_string(), "/work".to_string());
        self.mounts.insert(output_path.to_string_lossy().to_string(), "/output".to_string());

        let arch = appimage_arch();
        if self.image_cache.is_none() {
            // %{arch} in the appimage distro setup (the appimagetool download) is appimage-specific,
            // so it's substituted here rather than in the generic Distro::setup().
            self.commands.extend(self.distro.setup(&config.build_dependencies).into_iter().map(|c| c.replace("%{arch}", arch)));
            if let Some(bbs) = self.before_build_script("/source", &config) {
                self.commands.push(bbs);
            }
        }
        let rsync_excludes: String = self.ignore_source_files.iter().map(|p| format!(" --exclude='{p}'")).collect();
        let appimage_name = Self::appimage_name(&config.package_name);
        // The recipe runs with CWD /work and a writable source copy at /work/src; it must
        // compile and assemble /work/AppDir. appimagetool then turns it into the AppImage.
        // FUSE is unavailable in containers, so appimagetool runs in extract-and-run mode.
        self.commands.extend([
            format!("rsync -a{rsync_excludes} /source/ /work/src/"),
            "cd /work".to_string(),
            "bash /work/build-appimage.sh".to_string(),
            format!("APPIMAGE_EXTRACT_AND_RUN=1 ARCH={arch} appimagetool /work/AppDir /output/{appimage_name}"),
        ]);

        self.build_output_dir = output_path;
        self.setup_stages.push(SetupStage::Build);

        Ok(())
    }

    fn setup_repository(&mut self, config: Repository) -> Result<(), anyhow::Error> {
        // AppImage has no repo metadata and no native GPG-verify flow: just stage the file
        // (plus a .zsync sidecar when enabled). setup_repo_dir() creates the repository/
        // output dir; we deliberately skip prepare_gpgkey/prepare_repository.
        let repo_dir = self.setup_repo_dir()?;

        self.mounts.insert(repo_dir.to_string_lossy().to_string(), "/repo".to_string());
        self.mounts.insert(self.output_path().to_string_lossy().to_string(), "/output".to_string());

        let appimage_name = Self::appimage_name(&config.package_name);
        if self.zsync {
            // Re-run appimagetool from the AppDir (built in the same container during the
            // build stage) with update info, so the embedded URL and the emitted .zsync
            // match. zsync needs the absolute public URL, only known here.
            let update_url = format!("{}/{appimage_name}.zsync", self.distro_url(&config).trim_end_matches('/'));
            let arch = appimage_arch();
            // cd /repo: appimagetool delegates to `zsyncmake` without -o, which writes the
            // .zsync to the CWD (not next to the AppImage), so the sidecar must be generated
            // inside /repo to be uploaded. test -f fails the build if it wasn't produced.
            self.commands.push(format!(
                "cd /repo && APPIMAGE_EXTRACT_AND_RUN=1 ARCH={arch} appimagetool -u \"zsync|{update_url}\" /work/AppDir /repo/{appimage_name} && test -f /repo/{appimage_name}.zsync"
            ));
        } else {
            self.commands.push("cp /output/*.AppImage /repo/".to_string());
        }

        self.build_output_dir = repo_dir;
        self.setup_stages.push(SetupStage::Repository);
        // AppImage uses no GPG, but the install-page step reads gpgkey().pub_key; an empty
        // key satisfies it and renders as no key on the page.
        self.gpgkey = Some(Key {
            pub_key: String::new(),
            priv_key: String::new(),
        });

        Ok(())
    }

    fn clone_box(&self) -> Box<dyn Package> {
        Box::new(self.clone())
    }

    fn source_dir(&self) -> PathBuf {
        self.source_dir.clone()
    }

    fn distro_build_dir(&self) -> PathBuf {
        self.distro_build_dir.clone()
    }

    fn distro(&self) -> Distro {
        self.distro.clone()
    }

    fn mounts(&self) -> HashMap<String, String> {
        self.mounts.clone()
    }

    fn commands(&self) -> Vec<String> {
        self.commands.clone()
    }

    fn build_output_dir(&self) -> PathBuf {
        self.build_output_dir.clone()
    }

    fn setup_stages(&self) -> Vec<SetupStage> {
        self.setup_stages.clone()
    }

    fn gpgkey(&self) -> Option<Key> {
        self.gpgkey.clone()
    }

    fn image_cache(&self) -> Option<ImageCache> {
        self.image_cache.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppimageConfig, Build, Repository, RepositoryProvider, S3Config};
    use crate::distros::Distros;
    use crate::job_variables::JobVariables;
    use std::collections::HashMap;

    fn make_distro() -> Distro {
        Distros::get().by_id("appimage")
    }

    fn make_job_variables() -> JobVariables {
        JobVariables::new("1.2.3".to_string())
    }

    fn make_appimage(dir: &tempfile::TempDir) -> Appimage {
        let source_dir = dir.path().join("source");
        let build_dir = dir.path().join("build");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&build_dir).unwrap();
        Appimage::new(make_distro(), source_dir, make_job_variables(), build_dir, None, vec![])
    }

    fn make_build_config(dir: &tempfile::TempDir, zsync: bool) -> Build {
        let recipe_template = "appimage.sh.liquid";
        std::fs::write(dir.path().join("source").join(recipe_template), "#!/bin/bash\necho {{ package_name }} {{ version }}\n").unwrap();

        Build {
            distro: "appimage".to_string(),
            package_name: "myapp".to_string(),
            maintainer: "Test <test@test.com>".to_string(),
            homepage: "https://example.com".to_string(),
            description: "Test package".to_string(),
            build_dependencies: vec!["gcc".to_string(), "make".to_string()],
            runtime_dependencies: vec![],
            before_build_script: None,
            rpm: None,
            deb: None,
            pacman: None,
            appimage: Some(AppimageConfig {
                recipe_template: recipe_template.to_string(),
                zsync,
            }),
            rest: HashMap::new(),
        }
    }

    fn make_repository_config() -> Repository {
        Repository {
            name: "test-repo".to_string(),
            provider: RepositoryProvider::S3,
            localfs: None,
            s3: Some(S3Config {
                bucket: "test-bucket".to_string(),
                path_in_bucket: Some("packages".to_string()),
                bucket_public_url: Some("https://cdn.example.com".to_string()),
                endpoint: "https://s3.example.com".to_string(),
                access_key_id: "keyid".to_string(),
                secret_access_key: "secret".to_string(),
                region: Some("us-east-1".to_string()),
                force_path_style: false,
                cloudflare_zone_id: None,
                cloudflare_api_token: None,
            }),
            gpg_private_key_base64: String::new(),
            package_name: "myapp".to_string(),
            retain_packages: 0,
            rest: HashMap::new(),
        }
    }

    // ── new() ────────────────────────────────────────────────────────────────

    #[test]
    fn test_new_initializes_fields() {
        let dir = tempfile::tempdir().unwrap();
        let appimage = make_appimage(&dir);

        assert_eq!(appimage.distro().id, "appimage");
        assert!(appimage.mounts().is_empty());
        assert!(appimage.commands().is_empty());
        assert!(appimage.setup_stages().is_empty());
        assert!(appimage.gpgkey().is_none());
    }

    // ── setup_build() ────────────────────────────────────────────────────────

    #[test]
    fn test_setup_build_renders_recipe() {
        let dir = tempfile::tempdir().unwrap();
        let mut appimage = make_appimage(&dir);

        appimage.setup_build(make_build_config(&dir, false)).unwrap();

        let recipe = appimage.distro_build_dir().join("work").join("build-appimage.sh");
        assert!(recipe.exists());
        let content = std::fs::read_to_string(recipe).unwrap();
        assert!(content.contains("myapp 1.2.3"));
    }

    #[test]
    fn test_setup_build_adds_mounts() {
        let dir = tempfile::tempdir().unwrap();
        let mut appimage = make_appimage(&dir);

        appimage.setup_build(make_build_config(&dir, false)).unwrap();

        let mounts = appimage.mounts();
        assert!(mounts.values().any(|v| v == "/source"));
        assert!(mounts.values().any(|v| v == "/work"));
        assert!(mounts.values().any(|v| v == "/output"));
    }

    #[test]
    fn test_setup_build_adds_appimagetool_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut appimage = make_appimage(&dir);

        appimage.setup_build(make_build_config(&dir, false)).unwrap();

        let commands = appimage.commands();
        let name = format!("myapp-{}.AppImage", appimage_arch());
        assert!(commands.iter().any(|c| c.contains(&format!("appimagetool /work/AppDir /output/{name}"))));
        assert!(commands.iter().any(|c| c.contains("bash /work/build-appimage.sh")));
        assert!(commands.iter().any(|c| c.contains("rsync") && c.contains("/source/ /work/src/")));
    }

    #[test]
    fn test_setup_build_substitutes_arch_in_distro_setup() {
        let dir = tempfile::tempdir().unwrap();
        let mut appimage = make_appimage(&dir);

        appimage.setup_build(make_build_config(&dir, false)).unwrap();

        let commands = appimage.commands();
        assert!(commands.iter().any(|c| c.contains(&format!("appimagetool-{}.AppImage", appimage_arch()))));
        assert!(!commands.iter().any(|c| c.contains("%{arch}")));
    }

    #[test]
    fn test_setup_build_adds_build_to_stages() {
        let dir = tempfile::tempdir().unwrap();
        let mut appimage = make_appimage(&dir);

        appimage.setup_build(make_build_config(&dir, false)).unwrap();

        assert!(appimage.setup_stages().contains(&SetupStage::Build));
        assert!(appimage.build_output_dir().ends_with("output"));
    }

    #[test]
    fn test_setup_build_fails_without_appimage_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut appimage = make_appimage(&dir);
        let mut config = make_build_config(&dir, false);
        config.appimage = None;

        assert!(appimage.setup_build(config).is_err());
    }

    // ── setup_repository() ───────────────────────────────────────────────────

    #[test]
    fn test_setup_repository_copies_appimage_without_zsync() {
        let dir = tempfile::tempdir().unwrap();
        let mut appimage = make_appimage(&dir);
        appimage.setup_build(make_build_config(&dir, false)).unwrap();

        appimage.setup_repository(make_repository_config()).unwrap();

        let commands = appimage.commands();
        assert!(commands.iter().any(|c| c == "cp /output/*.AppImage /repo/"));
        assert!(!commands.iter().any(|c| c.contains("-u \"zsync|")));
    }

    #[test]
    fn test_setup_repository_injects_zsync_update_info() {
        let dir = tempfile::tempdir().unwrap();
        let mut appimage = make_appimage(&dir);
        appimage.setup_build(make_build_config(&dir, true)).unwrap();

        appimage.setup_repository(make_repository_config()).unwrap();

        let commands = appimage.commands();
        let name = format!("myapp-{}.AppImage", appimage_arch());
        let zsync_cmd = commands.iter().find(|c| c.contains("appimagetool -u")).expect("zsync appimagetool command not found");
        assert!(zsync_cmd.contains("zsync|"));
        assert!(zsync_cmd.contains(&format!("{name}.zsync")));
        assert!(zsync_cmd.contains(&format!("/work/AppDir /repo/{name}")));
        // zsyncmake writes the sidecar to the CWD, so it must run in /repo; the guard fails the build if it didn't.
        assert!(zsync_cmd.contains("cd /repo &&"));
        assert!(zsync_cmd.contains(&format!("test -f /repo/{name}.zsync")));
    }

    #[test]
    fn test_setup_repository_stores_empty_gpg_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut appimage = make_appimage(&dir);
        appimage.setup_build(make_build_config(&dir, false)).unwrap();

        appimage.setup_repository(make_repository_config()).unwrap();

        let key = appimage.gpgkey().expect("gpgkey should be Some");
        assert!(key.pub_key.is_empty());
        assert!(appimage.setup_stages().contains(&SetupStage::Repository));
    }

    // ── clone_box() ──────────────────────────────────────────────────────────

    #[test]
    fn test_clone_box() {
        let dir = tempfile::tempdir().unwrap();
        let appimage = make_appimage(&dir);
        let boxed: Box<dyn Package> = Box::new(appimage);
        let cloned = boxed.clone();

        assert_eq!(cloned.distro().id, boxed.distro().id);
        assert_eq!(cloned.source_dir(), boxed.source_dir());
    }
}
