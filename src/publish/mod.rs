use crate::PublishArgs;
use crate::config::{Config, Repository};
use crate::distros::{Distro, Distros};
use crate::gpg::{Gpg, Key};
use crate::logger::{Color, LogOutput, Logger, colorize};
use crate::shell::Command;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

mod deb;
mod rpm;
mod s3;

#[derive(Debug, Clone)]
pub struct PublishContext {
    pub distro: &'static Distro,
    args: PublishArgs,
    config: Repository,
    artefacts: Vec<PathBuf>,
    build_dir: PathBuf,
}

pub fn run(args: &PublishArgs) -> Result<(), String> {
    let config = Config::load_with_env(&args.project.source_dir.join(&args.project.config_path), &args.project.env_file)?;

    let repository_config = config.repositories.find_by_name_or_default(args.repository.as_deref())?;

    let contexts: Vec<PublishContext> = config
        .builds
        .iter()
        .filter(|build| Distros::get().contains(&build.distro))
        .filter(|build| args.distros.is_empty() || args.distros.contains(&build.distro))
        .filter_map(|build| {
            let build_dir = PathBuf::from(&args.build_dir).join(build.build_folder_name());
            if !build_dir.exists() {
                return None;
            }

            let distro = Distros::get().by_id(&build.distro);
            let artefacts = find_artefacts(distro, &build_dir);
            if artefacts.is_empty() {
                return None;
            }

            Some(PublishContext {
                distro,
                args: args.clone(),
                config: repository_config.clone(),
                artefacts,
                build_dir,
            })
        })
        .collect();

    Logger::new().info(format!(
        "found artefacts for {}",
        contexts.iter().map(|c| colorize(Color::BoldCyan, &c.distro.name)).collect::<Vec<_>>().join(", ")
    ));

    contexts.iter().for_each(|c| {
        c.run();
    });

    Ok(())
}

fn find_artefacts(distro: &'static Distro, build_dir: &Path) -> Vec<PathBuf> {
    let pattern = match distro.package_type.as_str() {
        "rpm" => build_dir.join("RPMS/**/*.rpm"),
        "deb" => build_dir.join("output").join("*.deb"), // NOTE: copy-paste, same logic happens in Package build
        _ => panic!("unknown package type {}", distro.package_type),
    };

    glob::glob(pattern.to_str().unwrap()).unwrap().filter_map(|e| e.ok()).collect()
}

impl PublishContext {
    pub fn run(&self) {
        Logger::new().info(format!("starting repository publish for {}", self.distro.id));

        let result = self.within_repository_dir(|dir| {
            match self.config.provider.as_str() {
                "s3" => {
                    let s3_config = self.config.s3();
                    let in_bucket_path = PathBuf::new()
                        .join(s3_config.path_in_bucket.as_deref().unwrap_or(""))
                        .join(&self.distro.id)
                        .to_string_lossy()
                        .to_string();
                    let s3 = s3::S3::new(s3_config, &in_bucket_path);

                    if !s3.bucket_exists()? {
                        return Err(format!("bucket '{}' does not exist", s3_config.bucket));
                    }
                    s3.download_all(dir)?;
                    self.setup_repo(dir)?;
                    s3.upload_all(dir)?;
                    s3.delete_deleted_files(dir)?;
                }
                &_ => todo!(),
            }

            Ok(())
        });

        match result {
            Ok(_) => Logger::new().info(format!("done repository publish for {}", self.distro.id)),
            Err(msg) => Logger::new().error(format!("error repository publish for {}: {}", self.distro.id, msg)),
        };
    }

    fn within_repository_dir<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Path) -> Result<R, String>,
    {
        let dir = self.build_dir.join("repository");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("cannot clear repository dir {}: {}", dir.display(), e))?;
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create repository dir {}: {}", dir.display(), e))?;
        f(&dir)
    }

    fn setup_repo(&self, dir: &Path) -> Result<(), String> {
        for artefact in &self.artefacts {
            let dest = dir.join(artefact.file_name().unwrap_or_else(|| artefact.as_os_str()));
            std::fs::copy(artefact, &dest).map_err(|e| format!("cannot copy {} to {}: {}", artefact.display(), dest.display(), e))?;
        }

        let home_dir_tempfile = tempfile::tempdir().expect("cannot create container home dir");
        let home_dir = home_dir_tempfile.path();

        let key = self.config.gpg_private_key()?;
        let gpg = Gpg::new();
        let gpgkey = gpg.test_private_key(&key).map_err(|e| format!("GPG key test failed: {}", e)).and_then(|_| gpg.key_from_private(&key))?;
        self.write_gpg_keys(&gpgkey, home_dir, dir)?;

        match self.distro.package_type.as_str() {
            "rpm" => self.setup_rpm_repo(&gpgkey, home_dir, dir),
            "deb" => self.setup_deb_repo(&gpgkey, home_dir, dir),
            _ => Err(format!("unknown package type {}", self.distro.package_type)),
        }
    }

    fn write_gpg_keys(&self, key: &Key, home_dir: &Path, work_dir: &Path) -> Result<(), String> {
        std::fs::write(work_dir.join("public.key"), &key.pub_key).map_err(|e| format!("cannot write public key: {}", e))?;
        std::fs::write(home_dir.join("key.priv"), &key.priv_key).map_err(|e| format!("cannot write private key: {}", e))?;
        Ok(())
    }

    fn import_gpg_keys_commands(&self) -> Vec<String> {
        vec!["gpg --no-tty --batch --import /root/key.priv".to_string(), "gpg --no-tty --batch --import public.key".to_string()]
    }

    fn execute(&self, commands: Vec<String>, home_dir: &Path, work_dir: &Path) -> Result<(), String> {
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--entrypoint".to_string(),
            "/bin/sh".to_string(),
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

        if !self.args.disable_container_echo {
            commands_with_setup.insert(0, "set -x".to_string());
        }
        // commands_with_setup.push("tree . ".to_string());

        args.push(self.distro.image.clone());
        args.push("-c".to_string());
        args.push(commands_with_setup.join(" && "));

        let log_path = self.build_dir.join("publish.log");
        let _ = std::fs::remove_file(&log_path);

        Command::container(args)
            .stream_output_to(self.container_logger())
            .log_to(&log_path)
            .run()
            .map_err(|code| format!("command failed with exit code {}", code))
    }

    fn container_logger(&self) -> Logger {
        let output = match self.args.container_output.as_str() {
            "stderr" => LogOutput::Stderr,
            "stdout" => LogOutput::Stdout,
            "null" => LogOutput::Silent,
            _ => LogOutput::Silent,
        };
        Logger::new().with_output(output)
    }
}
