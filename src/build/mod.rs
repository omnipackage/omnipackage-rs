use crate::config::{Build, Config};
use crate::distros::{Distro, Distros};
use crate::logger::{Color, LogOutput, Logger, colorize};
use crate::shell::Command;
use crate::{BuildArgs, JobArgs, LoggingArgs, ProjectArgs};
use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

pub mod extract_version;
pub mod job_variables;
mod package;

use job_variables::JobVariables;
use package::Package;

#[derive(Debug, Clone)]
pub struct BuildContext {
    pub distro: &'static Distro,
    pub source_dir: PathBuf,
    pub config: Build,
    pub job_variables: JobVariables,
    pub build_dir: PathBuf,
    pub logging_args: LoggingArgs,
}

type BuildError = (Box<dyn Error>, PathBuf);

impl BuildContext {
    pub fn run(&self) -> Result<(), Box<dyn Error>> {
        Logger::new().info(format!("starting build for {}, variables: {}", self.distro.id, self.job_variables));
        let started_at = Instant::now();
        let package = self.setup_package();
        let result = self.execute(&package);
        let finished_at = started_at.elapsed().as_secs_f32();
        match result {
            Ok((artefacts, build_log)) => {
                Logger::new().info(format!(
                    "successfully finished build for {} in {:.1}s, artefacts: {}",
                    self.distro.id,
                    finished_at,
                    colorize(Color::Green, artefacts.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")),
                ));

                Ok(())
            }
            Err((err, build_log)) => {
                Logger::new().error(format!(
                    "failed build for {} in {:.1}s ({}), log: {}",
                    self.distro.id,
                    finished_at,
                    err,
                    colorize(Color::Red, build_log.display())
                ));

                Err(err)
            }
        }
    }

    fn distro_build_dir(&self) -> PathBuf {
        self.build_dir.join(self.config.build_folder_name())
    }

    fn setup_package(&self) -> Package {
        match self.distro.package_type.as_str() {
            "rpm" => self.setup_rpm(),
            "deb" => self.setup_deb(),
            _ => panic!("unknown package type {}", self.distro.package_type),
        }
    }

    fn before_build_script(&self, relative_to: &str) -> Option<String> {
        let bbs = self.config.before_build_script.as_ref()?;

        let path = if self.source_dir.join(bbs).exists() {
            PathBuf::from(relative_to).join(bbs).to_string_lossy().to_string()
        } else {
            bbs.clone()
        };

        Some(path)
    }

    fn container_logger(&self) -> Logger {
        self.logging_args.container_logger().with_secrets(self.job_variables.secrets.values().cloned().collect::<Vec<String>>())
    }

    fn execute(&self, package: &Package) -> Result<(Vec<PathBuf>, PathBuf), BuildError> {
        let mut args = vec!["run".to_string(), "--rm".to_string(), "--entrypoint".to_string(), "/bin/bash".to_string()];

        let mut commands = package.commands.clone();
        if self.logging_args.disable_container_echo {
            commands.insert(0, "set -euo pipefail".to_string());
        } else {
            commands.insert(0, "set -euxo pipefail".to_string());
        }

        let mount_args: Vec<String> = package
            .mounts
            .iter()
            .flat_map(|(from, to)| ["--mount".to_string(), format!("type=bind,source={from},target={to}")])
            .collect();
        args.extend(mount_args);

        let env_args: Vec<String> = self.job_variables.secrets.iter().flat_map(|(k, v)| ["-e".to_string(), format!("{k}={v}")]).collect();
        args.extend(env_args);

        args.push(self.distro.image.clone());
        args.push("-c".to_string());
        args.push(commands.join(" && "));

        let log_path = self.build_dir.join(self.config.build_folder_name()).join("build.log");
        let _ = std::fs::remove_file(&log_path);

        Command::container(args)
            .stream_output_to(self.container_logger())
            .log_to(&log_path)
            .run()
            .map(|_| (package.artefacts(), log_path.clone()))
            .map_err(|err| (err, log_path.clone()))
    }
}
