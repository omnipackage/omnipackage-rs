use crate::LoggingArgs;
use crate::config::{Repository, S3Config};
use crate::logger::{Color, Logger, colorize};
use crate::package::Package;
use anyhow::{Context, Result};
use std::path::PathBuf;

mod artefacts;
mod cloudflare;
mod install_page;
mod s3;

use cloudflare::CloudflareApi;
use s3::S3;

pub struct Publish {
    pub logging: LoggingArgs,
    pub config: Repository,
    pub package: Box<dyn Package>,
    pub custom_install_page: Option<PathBuf>,
}

const INSTALL_PAGE_NAME: &str = "install.html";
const BADGE_NAME: &str = "badge.svg";

#[derive(Debug, Clone)]
struct InstallPageBadge {
    pub page_url: String,
    pub badge_md: String,
}

impl Publish {
    pub fn new(package: Box<dyn Package>, logging: LoggingArgs, config: Repository, custom_install_page: Option<PathBuf>) -> Self {
        Self {
            package,
            logging,
            config,
            custom_install_page,
        }
    }

    pub fn run(&self) -> Result<(), anyhow::Error> {
        Logger::new().info(format!("starting repository publish for {}", self.package.distro().id));

        if let Err(e) = self.sync_repo_files() {
            Logger::new().error(format!("failed publish for {} ({})", self.package.distro().id, e));
            return Err(e);
        }

        if let Err(e) = self.purge_cache() {
            Logger::new().warn(format!("cannot purge cache: {}", e));
        }

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

        Logger::new().info(format!("done repository publish for {}", self.package.distro().id));
        Ok(())
    }

    fn sync_repo_files(&self) -> Result<(), anyhow::Error> {
        let dir = self.package.repository_output_dir();
        if !dir.exists() {
            return Err(anyhow::anyhow!("repository dir does not exist: {}", self.package.repository_output_dir().display()));
        }
        if self.package.artefacts().is_empty() {
            return Err(anyhow::anyhow!("no artefacts in {}", self.package.build_output_dir().display()));
        }

        match self.config.provider.as_str() {
            "s3" => {
                let s3_config = self.config.s3();
                let s3 = S3::new(s3_config, self.s3_in_bucket_distro_path(s3_config));

                if !s3.bucket_exists()? {
                    return Err(anyhow::anyhow!("bucket '{}' does not exist", s3_config.bucket));
                }
                s3.upload_all(&dir)?;
                s3.delete_deleted_files(&dir)?;
                Ok(())
            }
            "localfs" => {
                let localfs_config = self.config.localfs();
                let dst = localfs_config.repository_path().join(&self.package.distro().id);
                artefacts::copy_dir_recursive(&dir, &dst)?;
                Ok(())
            }
            &_ => panic!("unknown repository provider {}", self.config.provider),
        }
    }

    fn s3_in_bucket_distro_path(&self, s3_config: &S3Config) -> String {
        PathBuf::new()
            .join(s3_config.path_in_bucket.as_deref().unwrap_or(""))
            .join(&self.package.distro().id)
            .to_string_lossy()
            .to_string()
    }

    fn purge_cache(&self) -> Result<(), anyhow::Error> {
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
        self.package.distro_url(&self.config)
    }

    fn install_steps(&self) -> Vec<String> {
        self.package
            .distro()
            .install_steps
            .iter()
            .map(|command| command.replace("%{package_name}", &self.config.package_name))
            .map(|command| command.replace("%{project_slug}", &self.config.project_slug()))
            .map(|command| command.replace("%{url}", &self.distro_url()))
            .collect()
    }

    fn update_install_page(&self) -> Result<InstallPageBadge, anyhow::Error> {
        let download_url = self.package_download_url()?;

        let repo = install_page::Repository::from([
            ("distro_id".to_string(), self.package.distro().id.clone()),
            ("distro_name".to_string(), self.package.distro().name.clone()),
            ("distro_family".to_string(), self.package.distro().family().to_string()),
            ("install_steps".to_string(), self.install_steps().join("\n")),
            ("gpg_key".to_string(), self.package.gpgkey().ok_or(anyhow::anyhow!("no gpg key"))?.pub_key),
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

                let custom_template = match self.custom_install_page.clone() {
                    Some(path) => Some(std::fs::read_to_string(path)?),
                    None => None,
                };

                let output = install_page::upsert(&existing_install_page, &repositories, &self.config, custom_template)?;
                s3.upload_file(INSTALL_PAGE_NAME, output.install_page.as_bytes().to_vec(), Some("text/html"))?;

                let page_url = install_page_url(&self.config).ok_or(anyhow::anyhow!("install page url cannot be generated"))?;
                s3.upload_file(BADGE_NAME, output.badge.as_bytes().to_vec(), Some("image/svg+xml"))?;

                let badge_url = format!("{}/{}", s3_config.base_bucket_url(), BADGE_NAME);
                let badge_md = format!("[![OmniPackage repositories badge]({badge_url})]({page_url})");

                Ok(InstallPageBadge { page_url, badge_md })
            }
            "localfs" => {
                let localfs_config = self.config.localfs();
                let path = localfs_config.repository_path().join(INSTALL_PAGE_NAME);
                let existing_install_page = std::fs::read_to_string(&path).unwrap_or_default();
                let custom_template = match self.custom_install_page.clone() {
                    Some(path) => Some(std::fs::read_to_string(path)?),
                    None => None,
                };
                let output = install_page::upsert(&existing_install_page, &repositories, &self.config, custom_template)?;
                std::fs::write(&path, output.install_page)?;

                Ok(InstallPageBadge {
                    page_url: path.to_string_lossy().to_string(),
                    badge_md: "".to_string(),
                })
            }
            &_ => panic!("unknown repository provider {}", self.config.provider),
        }
    }

    fn package_download_url(&self) -> Result<String, anyhow::Error> {
        let dir = self.package.repository_output_dir();
        let package_files = artefacts::find_artefacts_in_repository_dir(&self.package.artefacts(), &dir).with_context(|| anyhow::anyhow!("cannot find packages in {}", dir.display()))?;
        let package_file = package_files.first().ok_or_else(|| anyhow::anyhow!("no packages found in repository dir"))?;

        Ok(format!("{}/{}", self.distro_url().trim_end_matches('/'), package_file.relative_path.display()))
    }
}

pub fn install_page_url(repository: &Repository) -> Option<String> {
    match repository.provider.as_str() {
        "s3" => {
            let s3_config = repository.s3();
            let page_url = format!("{}/{}", s3_config.base_bucket_url(), INSTALL_PAGE_NAME);
            Some(page_url)
        }
        &_ => None,
    }
}
