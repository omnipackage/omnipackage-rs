use crate::config::{Build, Repository, S3Config};
use crate::distros::Distro;
use crate::gpg::{Gpg, Key};
use crate::job_variables::JobVariables;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod deb;
pub mod rpm;

impl Clone for Box<dyn Package> {
    fn clone(&self) -> Box<dyn Package> {
        self.as_ref().clone_box()
    }
}

pub fn make_package(distro: &'static Distro, source_dir: PathBuf, job_variables: JobVariables, distro_build_dir: PathBuf) -> Result<Box<dyn Package>, anyhow::Error> {
    match distro.package_type.as_str() {
        "deb" => Ok(Box::new(deb::Deb::new(distro, source_dir, job_variables, distro_build_dir))),
        "rpm" => Ok(Box::new(rpm::Rpm::new(distro, source_dir, job_variables, distro_build_dir))),
        other => Err(anyhow::anyhow!("unknown package type: {other}")),
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum SetupStage {
    Build,
    Repository,
}

pub trait Package {
    fn clone_box(&self) -> Box<dyn Package>;

    fn setup_build(&mut self, config: Build) -> Result<(), anyhow::Error>;
    fn setup_repository(&mut self, config: Repository) -> Result<(), anyhow::Error>;

    fn mounts(&self) -> HashMap<String, String>;
    fn commands(&self) -> Vec<String>;
    fn source_dir(&self) -> PathBuf;
    fn distro_build_dir(&self) -> PathBuf;
    fn distro(&self) -> &'static Distro;
    fn build_output_dir(&self) -> PathBuf;
    fn setup_stages(&self) -> Vec<SetupStage>;
    fn gpgkey(&self) -> Option<Key>;

    fn teardown(&self) {
        let dir = self.home_dir();
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    fn setup_stage_name(&self) -> String {
        let s = self.setup_stages();
        if s.contains(&SetupStage::Build) && s.contains(&SetupStage::Repository) {
            "build & respository setup".to_string()
        } else if s.contains(&SetupStage::Build) {
            "build".to_string()
        } else if s.contains(&SetupStage::Repository) {
            "respository setup".to_string()
        } else {
            "<empty package preparation stage>".to_string()
        }
    }

    fn artefacts(&self) -> Vec<PathBuf> {
        let ext = match self.distro().package_type.as_str() {
            "rpm" => "rpm",
            "deb" => "deb",
            other => panic!("unknown package type {}", other),
        };

        let stage = self.setup_stages();
        let dir = if stage.contains(&SetupStage::Repository) {
            self.repository_output_dir()
        } else if stage.contains(&SetupStage::Build) {
            self.build_output_dir()
        } else {
            panic!("package not set up")
        };

        let pattern = dir.join(format!("**/*.{}", ext));

        glob::glob(pattern.to_str().unwrap()).unwrap().filter_map(|e| e.ok()).collect()
    }

    fn before_build_script(&self, relative_to: &str, config: &Build) -> Option<String> {
        let bbs = config.before_build_script.as_ref()?;

        let path = if self.source_dir().join(bbs).exists() {
            PathBuf::from(relative_to).join(bbs).to_string_lossy().to_string()
        } else {
            bbs.clone()
        };

        Some(path)
    }

    fn import_gpg_keys_commands(&self) -> Vec<String> {
        vec!["gpg --no-tty --batch --import /root/key.priv".to_string(), "gpg --no-tty --batch --import /repo/public.key".to_string()]
    }

    fn write_gpg_keys(&self, key: &Key, home_dir: &Path, repo_dir: &Path) -> Result<(), anyhow::Error> {
        std::fs::write(repo_dir.join("public.key"), &key.pub_key)?;
        std::fs::write(home_dir.join("key.priv"), &key.priv_key)?;
        Ok(())
    }

    fn prepare_repository(&self, gpgkey: &Key) -> Result<(PathBuf, PathBuf), anyhow::Error> {
        let home_dir = self.setup_home_dir()?;
        let repo_dir = self.setup_repo_dir()?;

        self.write_gpg_keys(gpgkey, &home_dir, &repo_dir)?;

        Ok((home_dir, repo_dir))
    }

    fn prepare_gpgkey(&self, config: &Repository) -> Result<Key, anyhow::Error> {
        let gpg = Gpg::new();
        let key = &config.gpg_private_key()?;
        gpg.test_private_key(key).with_context(|| "GPG key test failed".to_string())?;
        gpg.key_from_private(key)
    }

    fn repository_output_dir(&self) -> PathBuf {
        self.distro_build_dir().join("repository")
    }

    fn home_dir(&self) -> PathBuf {
        self.distro_build_dir().join("home")
    }

    fn setup_repo_dir(&self) -> Result<PathBuf, anyhow::Error> {
        let dir = self.repository_output_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn setup_home_dir(&self) -> Result<PathBuf, anyhow::Error> {
        let dir = self.home_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn prepare_build_dir(&self) -> Result<PathBuf, anyhow::Error> {
        let dir = self.distro_build_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn distro_url(&self, config: &Repository) -> String {
        match config.provider.as_str() {
            "s3" => {
                let s3_config = config.s3();
                format!("{}/{}", s3_config.base_url(), self.s3_in_bucket_distro_path(s3_config))
            }
            &_ => todo!(),
        }
    }

    fn s3_in_bucket_distro_path(&self, s3_config: &S3Config) -> String {
        PathBuf::new()
            .join(s3_config.path_in_bucket.as_deref().unwrap_or(""))
            .join(&self.distro().id)
            .to_string_lossy()
            .to_string()
    }
}
