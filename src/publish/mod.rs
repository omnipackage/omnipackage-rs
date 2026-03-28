use crate::artefacts;
use crate::config::{Config, Repository, S3Config};
use crate::distros::{Distro, Distros};
use crate::gpg::{Gpg, Key};
use crate::logger::{Color, LogOutput, Logger, colorize};
use crate::shell::Command;
use crate::template::{Template, Var};
use crate::{JobArgs, LoggingArgs, ProjectArgs, PublishArgs};
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

mod deb;
mod install_page;
mod rpm;
mod s3;

#[derive(Debug, Clone)]
pub struct PublishContext {
    pub distro: &'static Distro,
    pub logging_args: LoggingArgs,
    pub config: Repository,
    pub artefacts: Vec<PathBuf>,
    pub build_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct SetupRepoOutput {
    pub gpgkey: Key,
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
struct InstallPageBadge {
    pub page_url: String,
    pub badge_md: String,
}

#[derive(Debug)]
pub struct ErrorWithLog {
    pub error: Box<dyn Error>,
    pub log_path: PathBuf,
}

impl Error for ErrorWithLog {}

impl std::fmt::Display for ErrorWithLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (log: {})", self.error, self.log_path.display())
    }
}

impl ErrorWithLog {
    pub fn wrap(error: Box<dyn Error>, log_path: PathBuf) -> Box<dyn Error> {
        Box::new(Self { error, log_path })
    }
}

impl PublishContext {
    pub fn run(&self) -> Result<(), Box<dyn Error>> {
        Logger::new().info(format!("starting repository publish for {}", self.distro.id));

        let output = self.run_publish().map_err(|e| {
            if let Some(e) = e.downcast_ref::<ErrorWithLog>() {
                Logger::new().error(format!("failed publish for {} ({}), log: {}", self.distro.id, e.error, colorize(Color::Red, e.log_path.display())));
            } else {
                Logger::new().error(format!("failed publish for {} ({})", self.distro.id, e));
            }
            e
        })?;

        Logger::new().info(format!("done repository publish for {}", self.distro.id));

        let res = self.update_install_page(output).map_err(|e| {
            Logger::new().error(format!("failed deploy install page for {} ({})", self.distro.id, e));
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

    fn run_publish(&self) -> Result<SetupRepoOutput, Box<dyn Error>> {
        if !self.build_dir.exists() {
            return Err(format!("distro build dir does not exist: {}", self.build_dir.display()).into());
        }
        if self.artefacts.is_empty() {
            return Err(format!("no artefacts in {}", self.build_dir.display()).into());
        }

        self.within_repository_dir(|dir| match self.config.provider.as_str() {
            "s3" => {
                let s3_config = self.config.s3();
                let s3 = s3::S3::new(s3_config, self.s3_in_bucket_distro_path(s3_config));

                if !s3.bucket_exists()? {
                    return Err(format!("bucket '{}' does not exist", s3_config.bucket).into());
                }
                let output = self.setup_repo(dir)?;
                s3.upload_all(dir)?;
                s3.delete_deleted_files(dir)?;
                Ok(output)
            }
            &_ => todo!(),
        })
    }

    fn within_repository_dir<F, R>(&self, f: F) -> Result<R, Box<dyn Error>>
    where
        F: FnOnce(&Path) -> Result<R, Box<dyn Error>>,
    {
        let dir = self.build_dir.join("repository");
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        f(&dir)
    }

    fn setup_repo(&self, dir: &Path) -> Result<SetupRepoOutput, Box<dyn Error>> {
        for artefact in &self.artefacts {
            let dest = dir.join(artefact.file_name().unwrap_or_else(|| artefact.as_os_str()));
            std::fs::copy(artefact, &dest).map_err(|e| format!("cannot copy {} to {}: {}", artefact.display(), dest.display(), e))?;
        }

        let home_dir_tempfile = tempfile::tempdir().expect("cannot create container home dir");
        let home_dir = home_dir_tempfile.path();

        let key = self.config.gpg_private_key()?;
        let gpg = Gpg::new();
        gpg.test_private_key(&key).map_err(|e| format!("GPG key test failed: {}", e))?;
        let gpgkey = gpg.key_from_private(&key).map_err(|e| e.to_string())?;
        self.write_gpg_keys(&gpgkey, home_dir, dir)?;

        match self.distro.package_type.as_str() {
            "rpm" => {
                self.setup_rpm_repo(&gpgkey, home_dir, dir)?;
                Ok(SetupRepoOutput { gpgkey, dir: dir.to_path_buf() })
            }
            "deb" => {
                self.setup_deb_repo(&gpgkey, home_dir, dir)?;
                Ok(SetupRepoOutput { gpgkey, dir: dir.to_path_buf() })
            }
            _ => Err(format!("unknown package type {}", self.distro.package_type).into()),
        }
    }

    fn write_gpg_keys(&self, key: &Key, home_dir: &Path, work_dir: &Path) -> Result<(), Box<dyn Error>> {
        std::fs::write(work_dir.join("public.key"), &key.pub_key)?;
        std::fs::write(home_dir.join("key.priv"), &key.priv_key)?;
        Ok(())
    }

    fn import_gpg_keys_commands(&self) -> Vec<String> {
        vec!["gpg --no-tty --batch --import /root/key.priv".to_string(), "gpg --no-tty --batch --import public.key".to_string()]
    }

    fn execute(&self, commands: Vec<String>, home_dir: &Path, work_dir: &Path) -> Result<(), Box<dyn Error>> {
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--entrypoint".to_string(),
            "/bin/bash".to_string(),
            "--workdir".to_string(),
            "/workdir".to_string(),
        ];

        let mounts: HashMap<String, String> = [
            (work_dir.to_string_lossy().to_string(), "/workdir".to_string()),
            (home_dir.to_string_lossy().to_string(), "/root".to_string()),
        ]
        .into_iter()
        .collect();
        let mount_args: Vec<String> = mounts.iter().flat_map(|(from, to)| ["--mount".to_string(), format!("type=bind,source={from},target={to}")]).collect();
        args.extend(mount_args);

        let env_vars: HashMap<String, String> = [("GPG_TTY".to_string(), "".to_string())].into_iter().collect();
        let env_args: Vec<String> = env_vars.iter().flat_map(|(k, v)| ["-e".to_string(), format!("{k}={v}")]).collect();
        args.extend(env_args);

        let mut commands_with_setup = self.distro.setup_repo.clone();
        commands_with_setup.extend(commands);

        if self.logging_args.disable_container_echo {
            commands_with_setup.insert(0, "set -euo pipefail".to_string());
        } else {
            commands_with_setup.insert(0, "set -euxo pipefail".to_string());
        }

        args.push(self.distro.image.clone());
        args.push("-c".to_string());
        args.push(commands_with_setup.join(" && "));

        let log_path = self.build_dir.join("publish.log");
        let _ = std::fs::remove_file(&log_path);

        Command::container(args)
            .stream_output_to(self.logging_args.container_logger())
            .log_to(&log_path)
            .run()
            .map_err(|e| ErrorWithLog::wrap(e, log_path.clone()))
    }

    fn s3_in_bucket_distro_path(&self, s3_config: &S3Config) -> String {
        PathBuf::new()
            .join(s3_config.path_in_bucket.as_deref().unwrap_or(""))
            .join(&self.distro.id)
            .to_string_lossy()
            .to_string()
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
        self.distro
            .install_steps
            .iter()
            .map(|command| command.replace("%{package_name}", &self.config.package_name))
            .map(|command| command.replace("%{project_slug}", &self.config.project_slug()))
            .map(|command| command.replace("%{url}", &self.distro_url()))
            .collect()
    }

    fn update_install_page(&self, setup_repo_output: SetupRepoOutput) -> Result<InstallPageBadge, Box<dyn Error>> {
        const INSTALL_PAGE_NAME: &str = "install.html";
        const BADGE_NAME: &str = "badge.svg";

        let download_url = self.package_download_url(&setup_repo_output)?;

        let repo = install_page::Repository::from([
            ("distro_id".to_string(), self.distro.id.clone()),
            ("distro_name".to_string(), self.distro.name.clone()),
            ("distro_family".to_string(), self.distro.family().to_string()),
            ("install_steps".to_string(), self.install_steps().join("\n")),
            ("gpg_key".to_string(), setup_repo_output.gpgkey.pub_key),
            ("download_url".to_string(), download_url),
            ("package_type".to_string(), self.distro.package_type.clone()),
        ]);
        let repositories: install_page::Repositories = vec![repo];

        match self.config.provider.as_str() {
            "s3" => {
                let s3_config = self.config.s3();
                let path = PathBuf::new().join(s3_config.path_in_bucket.as_deref().unwrap_or(""));
                let s3 = s3::S3::new(s3_config, path.to_string_lossy().to_string());

                let existing_page_bytes = s3.download_file(INSTALL_PAGE_NAME).unwrap_or(vec![]);
                let existing_install_page = String::from_utf8_lossy(&existing_page_bytes).into_owned();
                let output = install_page::upsert(&existing_install_page, &repositories, self.config.to_template_vars())?;

                s3.upload_file(INSTALL_PAGE_NAME, output.install_page.as_bytes().to_vec(), Some("text/html"))?;

                let page_url = format!("{}/{}", s3_config.base_bucket_url(), INSTALL_PAGE_NAME);
                s3.upload_file(BADGE_NAME, output.badge.as_bytes().to_vec(), Some("image/svg+xml"))?;

                let badge_url = format!("{}/{}", s3_config.base_bucket_url(), BADGE_NAME);
                let badge_md = format!("[![OmniPackage repositories badge]({badge_url})]({page_url})");

                Ok(InstallPageBadge { page_url, badge_md })
            }
            &_ => todo!(),
        }
    }

    fn package_download_url(&self, setup_repo_output: &SetupRepoOutput) -> Result<String, Box<dyn Error>> {
        let package_files = artefacts::find_artefacts_in_repository_dir(&self.artefacts, &setup_repo_output.dir).map_err(|e| format!("cannot find packages in repository dir: {e}"))?;
        let package_file = package_files.first().ok_or_else(|| "no packages found in repository dir".to_string())?;

        Ok(format!("{}/{}", self.distro_url().trim_end_matches('/'), package_file.relative_path.display()))
    }
}
