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
use crate::publish::install_page;

pub struct Publisher {
    pub logging: LoggingArgs,
    pub config: Repository,
    pub package: Box<dyn Package>,
}

const INSTALL_PAGE_NAME: &str = "install.html";
const BADGE_NAME: &str = "badge.svg";

#[derive(Debug, Clone)]
struct InstallPageBadge {
    pub page_url: String,
    pub badge_md: String,
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

        let res = self.update_install_page().map_err(|e| {
            Logger::new().error(format!("failed deploy install page for {} ({})", self.package.distro().id, e));
            e
        })?;

        if !res.page_url.is_empty() {
            Logger::new().info(format!("install page: {}", colorize(Color::Green, res.page_url)));
        }
        if !res.badge_md.is_empty() {
            Logger::new().info(format!("badge markdown: {}", colorize(Color::Yellow, res.badge_md)));
        }

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

    fn distro_url(&self) -> String {
        match self.config.provider.as_str() {
            "s3" => {
                let s3_config = self.config.s3();
                format!("{}/{}", s3_config.base_url(), self.s3_in_bucket_distro_path(s3_config))
            }
            &_ => todo!(),
        }
    }

    fn install_steps(&self) -> Vec<String> {
        self.package.distro()
            .install_steps
            .iter()
            .map(|command| command.replace("%{package_name}", &self.config.package_name))
            .map(|command| command.replace("%{project_slug}", &self.config.project_slug()))
            .map(|command| command.replace("%{url}", &self.distro_url()))
            .collect()
    }

    fn update_install_page(&self) -> Result<InstallPageBadge, Box<dyn Error>> {
        let download_url = self.package_download_url()?;

        let repo = install_page::Repository::from([
            ("distro_id".to_string(), self.package.distro().id.clone()),
            ("distro_name".to_string(), self.package.distro().name.clone()),
            ("distro_family".to_string(), self.package.distro().family().to_string()),
            ("install_steps".to_string(), self.install_steps().join("\n")),
            ("gpg_key".to_string(), "TODO".to_string()),
            ("download_url".to_string(), download_url),
            ("package_type".to_string(), self.package.distro().package_type.clone()),
            ("timestamp".to_string(), chrono::Utc::now().to_rfc3339()),
        ]);
        let repositories: install_page::Repositories = vec![repo];

        match self.config.provider.as_str() {
            "s3" => {
                let s3_config = self.config.s3();
                let path = PathBuf::new().join(s3_config.path_in_bucket.as_deref().unwrap_or(""));
                let s3 = S3::new(s3_config, path.to_string_lossy().to_string());

                let existing_page_bytes = s3.download_file(INSTALL_PAGE_NAME).unwrap_or(vec![]);
                let existing_install_page = String::from_utf8_lossy(&existing_page_bytes).into_owned();
                let output = install_page::upsert(&existing_install_page, &repositories, &self.config)?;

                s3.upload_file(INSTALL_PAGE_NAME, output.install_page.as_bytes().to_vec(), Some("text/html"))?;

                let page_url = self.install_page_url().ok_or("install page url cannot be generated")?;
                s3.upload_file(BADGE_NAME, output.badge.as_bytes().to_vec(), Some("image/svg+xml"))?;

                let badge_url = format!("{}/{}", s3_config.base_bucket_url(), BADGE_NAME);
                let badge_md = format!("[![OmniPackage repositories badge]({badge_url})]({page_url})");

                Ok(InstallPageBadge { page_url, badge_md })
            }
            &_ => todo!(),
        }
    }

    fn package_download_url(&self) -> Result<String, Box<dyn Error>> {
        let package_files = artefacts::find_artefacts_in_repository_dir(&self.package.artefacts(), &self.package.repository_output_dir()).map_err(|e| format!("cannot find packages in repository dir: {e}"))?;
        let package_file = package_files.first().ok_or_else(|| "no packages found in repository dir".to_string())?;

        Ok(format!("{}/{}", self.distro_url().trim_end_matches('/'), package_file.relative_path.display()))
    }

    fn install_page_url(&self) -> Option<String> {
        match self.config.provider.as_str() {
            "s3" => {
                let s3_config = self.config.s3();
                let page_url = format!("{}/{}", s3_config.base_bucket_url(), INSTALL_PAGE_NAME);
                Some(page_url)
            }
            &_ => None,
        }
    }
}
