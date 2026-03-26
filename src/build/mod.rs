use crate::config::{Build, Config};
use crate::distros::{Distro, Distros};
use crate::logger::{LogOutput, Logger};
use crate::shell::Command;
use crate::{BuildArgs, JobArgs, LoggingArgs, ProjectArgs};
use std::path::PathBuf;
use std::result::Result;
use std::time::Instant;

mod extract_version;
mod job_variables;
pub mod output;
mod package;

use job_variables::JobVariables;
use output::Output;
use package::Package;

pub fn run(project: &ProjectArgs, job: &JobArgs, logging: &LoggingArgs) -> Result<Vec<Output>, String> {
    let config = project.load_config()?;

    let version = extract_version::extract_version(&project.source_dir, &config.extract_version);
    let job_variables = JobVariables::build(version.clone()).with_secrets(config.secrets.clone().into_iter().collect());

    let outputs = config
        .builds
        .iter()
        .filter(|build| Distros::get().contains(&build.distro))
        .filter(|build| job.distros.is_empty() || job.distros.contains(&build.distro))
        .map(|build| {
            BuildContext {
                distro: Distros::get().by_id(&build.distro),
                source_dir: project.source_dir.clone(),
                config: build.clone(),
                job_variables: job_variables.clone(),
                build_dir: PathBuf::from(&job.build_dir),
                logging_args: logging.clone(),
            }
            .run()
        })
        .collect();
    Ok(outputs)
}

#[derive(Debug, Clone)]
pub struct BuildContext {
    pub distro: &'static Distro,
    pub source_dir: PathBuf,
    pub config: Build,
    pub job_variables: JobVariables,
    pub build_dir: PathBuf,
    pub logging_args: LoggingArgs,
}

impl BuildContext {
    pub fn run(&self) -> Output {
        Logger::new().info(format!("starting build for {}, variables: {}", self.distro.id, self.job_variables));
        let started_at = Instant::now();
        let package = self.setup_package();
        let result = self.execute(&package);
        let finished_at = started_at.elapsed().as_secs_f32();
        match result {
            Ok((artefacts, build_log)) => {
                Logger::new().info(format!(
                    "successfully finished build for {} in {:.1}s, artefacts: {:?}, log: {}",
                    self.distro.id,
                    finished_at,
                    artefacts,
                    build_log.display()
                ));

                Output {
                    success: true,
                    artefacts,
                    build_log,
                    distro: self.distro,
                    distro_build_dir: self.distro_build_dir(),
                }
            }
            Err((_code, build_log)) => {
                Logger::new().error(format!("failed build for {} in {:.1}s, log: {}", self.distro.id, finished_at, build_log.display()));

                Output {
                    success: false,
                    artefacts: Vec::new(),
                    build_log,
                    distro: self.distro,
                    distro_build_dir: self.distro_build_dir(),
                }
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

    fn execute(&self, package: &Package) -> Result<(Vec<PathBuf>, PathBuf), (i32, PathBuf)> {
        let mut args = vec!["run".to_string(), "--rm".to_string(), "--entrypoint".to_string(), "/bin/sh".to_string()];

        let mut commands = package.commands.clone();
        if !self.logging_args.disable_container_echo {
            commands.insert(0, "set -x".to_string());
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
            .map_err(|code| (code, log_path.clone()))
    }
}
