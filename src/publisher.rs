use crate::LoggingArgs;
use crate::artefacts;
use crate::config::{Repository, S3Config};
use crate::distros::Distro;
use crate::gpg::{Gpg, Key};
use crate::logger::{Color, Logger, colorize};
use crate::shell::Command;
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use crate::publish::s3::S3;
use crate::publish::cloudflare::CloudflareApi;
use crate::package::Package;

pub struct Publisher {
    pub logging: LoggingArgs,
    pub config: Repository,
    pub package: Box<dyn Package>,
}

impl Publisher {
    pub fn new(package: Box<dyn Package>, logging: LoggingArgs, config: Repository) -> Self {
        Self { package, logging, config }
    }

    pub fn run(&self) -> Result<(), Box<dyn Error>> {
        Logger::new().info(format!("starting repository publish for {}", self.package.distro().id));

        if let Err(e) = self.sync_repo_files() {
            Logger::new().error(format!("failed publish for {} ({})", self.package.distro().id, e));
            return Err(e);
        }

        if let Err(e) = self.purge_cache() {
            Logger::new().warn(format!("cannot purge cache: {}", e));
        }

        Logger::new().info(format!("done repository publish for {}", self.package.distro().id));

        Ok(())
    }

    fn sync_repo_files(&self) -> Result<(), Box<dyn Error>> {
        let dir = self.package.repository_output_dir();
        if !dir.exists() {
            return Err(format!("repository dir does not exist: {}", self.package.repository_output_dir().display()).into());
        }
        if self.package.artefacts().is_empty() {
            return Err(format!("no artefacts in {}", self.package.build_output_dir().display()).into());
        }

         match self.config.provider.as_str() {
            "s3" => {
                let s3_config = self.config.s3();
                let s3 = S3::new(s3_config, self.s3_in_bucket_distro_path(s3_config));

                if !s3.bucket_exists()? {
                    return Err(format!("bucket '{}' does not exist", s3_config.bucket).into());
                }
                s3.upload_all(&dir)?;
                s3.delete_deleted_files(&dir)?;
                Ok(())
            }
            &_ => todo!(),
        }
    }

    fn s3_in_bucket_distro_path(&self, s3_config: &S3Config) -> String {
        PathBuf::new()
            .join(s3_config.path_in_bucket.as_deref().unwrap_or(""))
            .join(&self.package.distro().id)
            .to_string_lossy()
            .to_string()
    }

    fn purge_cache(&self) -> Result<(), Box<dyn Error>> {
        if self.config.provider.as_str() != "s3" {
            return Ok(());
        }
        let s3_config = self.config.s3();

        let binding = s3_config.base_bucket_url();
        let prefix = binding.trim_start_matches("https://").trim_start_matches("http://");

        if let (Some(zone_id), Some(api_token)) = (&s3_config.cloudflare_zone_id, &s3_config.cloudflare_api_token) {
            Logger::new().info(format!("purging Cloudflare CDN cache at {}", prefix));
            CloudflareApi::new(zone_id.clone(), api_token.clone()).purge_by_prefix(prefix)?;
        }
        Ok(())
    }
}
