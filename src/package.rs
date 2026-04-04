use crate::LoggingArgs;
use crate::distros::Distro;
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use crate::config::{Build, Repository, S3Config};
use crate::gpg::{Key, Gpg};

pub mod rpm;
pub mod deb;

pub trait Package {
    fn build(&mut self) -> Result<(), Box<dyn Error>>;
    fn publish(&mut self) -> Result<(), Box<dyn Error>>;

    fn mounts(&self) -> HashMap<String, String>;
    fn commands(&self) -> Vec<String>;

    fn source_dir(&self) -> PathBuf;
    fn build_config(&self) -> Build;
    fn repository_config(&self) -> Repository;
    fn build_dir(&self) -> PathBuf;
    fn distro(&self) -> &'static Distro;

    fn before_build_script(&self, relative_to: &str) -> Option<String> {
        let cfg = self.build_config();
        let bbs = cfg.before_build_script.as_ref()?;

        let path = if self.source_dir().join(bbs).exists() {
            PathBuf::from(relative_to).join(bbs).to_string_lossy().to_string()
        } else {
            bbs.clone()
        };

        Some(path)
    }

    fn distro_build_dir(&self) -> PathBuf {
        self.build_dir().join(self.build_config().build_folder_name())
    }

    fn import_gpg_keys_commands(&self) -> Vec<String> {
        vec!["gpg --no-tty --batch --import /root/key.priv".to_string(), "gpg --no-tty --batch --import public.key".to_string()]
    }

    fn write_gpg_keys(&self, key: &Key, home_dir: &Path, repo_dir: &Path) -> Result<(), Box<dyn Error>> {
        std::fs::write(repo_dir.join("public.key"), &key.pub_key)?;
        std::fs::write(home_dir.join("key.priv"), &key.priv_key)?;
        Ok(())
    }

    fn publish_prepare(&self) -> Result<(), Box<dyn Error>> {
        let home_dir = self.setup_home_dir()?;
        let repo_dir = self.setup_repo_dir()?;

        let key = self.repository_config().gpg_private_key()?;
        let gpg = Gpg::new();
        gpg.test_private_key(&key).map_err(|e| format!("GPG key test failed: {}", e))?;
        let gpgkey = gpg.key_from_private(&key).map_err(|e| e.to_string())?;
        self.write_gpg_keys(&gpgkey, &home_dir, &repo_dir)?;

        Ok(())
    }

    fn publish_mounts(&self) -> HashMap<String, String> {
        [
            (self.repo_dir().to_string_lossy().to_string(), "/repo".to_string()),
            (self.home_dir().to_string_lossy().to_string(), "/root".to_string()),
        ].into()
    }

    fn repo_dir(&self) -> PathBuf {
        self.distro_build_dir().join("repository")
    }

    fn home_dir(&self) -> PathBuf {
        self.distro_build_dir().join("home")
    }

    fn setup_repo_dir(&self) -> Result<PathBuf, Box<dyn Error>> {
        let dir = self.repo_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn setup_home_dir(&self) -> Result<PathBuf, Box<dyn Error>> {
        let dir = self.home_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn distro_url(&self) -> String {
        let repo = self.repository_config();
        match repo.provider.as_str() {
            "s3" => {
                let s3_config = repo.s3();
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
