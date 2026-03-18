use crate::BuildArgs;
use crate::config::{Build, Config};
use crate::distros::{Distro, Distros};
use crate::shell::{Command, StreamOutput};
use std::path::PathBuf;
use std::result::Result;
use std::time::Instant;

mod extract_version;
mod job_variables;
pub mod output;
pub mod package;

use job_variables::JobVariables;
use output::Output;
use package::Package;

pub fn run(args: &BuildArgs) -> Vec<Output> {
    let config = Config::load(&args.source_path.join(".omnipackage/config.yml"));

    let version = extract_version::extract_version(&args.source_path, &config.extract_version);
    let job_variables = JobVariables::build(version);
    // TODO: add secrets
    // TODO: add limits

    config
        .builds
        .iter()
        .filter(|build| Distros::get().contains(&build.distro))
        .filter(|build| args.distros.is_empty() || args.distros.contains(&build.distro))
        .map(|build| {
            BuildContext {
                distro: Distros::get().by_id(&build.distro),
                source_path: args.source_path.clone(),
                config: build.clone(),
                job_variables: job_variables.clone(),
                build_dir: PathBuf::from(&args.build_dir),
            }
            .run()
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct BuildContext {
    pub distro: &'static Distro,
    pub source_path: PathBuf,
    pub config: Build,
    pub job_variables: JobVariables,
    pub build_dir: PathBuf,
}

impl BuildContext {
    pub fn run(&self) -> Output {
        crate::logger::info(format!(
            "starting build for {} at {}, variables: {}",
            self.distro.id,
            self.source_path.display(),
            self.job_variables
        ));
        let started_at = Instant::now();
        let package = self.setup_package();
        let result = self.execute(&package);
        let finished_at = started_at.elapsed().as_secs_f32();
        match result {
            Ok((artefacts, build_log)) => {
                crate::logger::info(format!(
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
                }
            }
            Err((_code, build_log)) => {
                crate::logger::error(format!("failed build for {} in {:.1}s, log: {}", self.distro.id, finished_at, build_log.display()));

                Output {
                    success: false,
                    artefacts: Vec::new(),
                    build_log,
                    distro: self.distro,
                }
            }
        }
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

        let path = if self.source_path.join(bbs).exists() {
            PathBuf::from(relative_to).join(bbs).to_string_lossy().to_string()
        } else {
            bbs.clone()
        };

        Some(path)
    }

    fn execute(&self, package: &Package) -> Result<(Vec<PathBuf>, PathBuf), (i32, PathBuf)> {
        let mut args = vec!["run".to_string(), "--rm".to_string(), "--entrypoint".to_string(), "/bin/sh".to_string()];

        let mut commands = package.commands.clone();
        commands.insert(0, "set -x".to_string()); // TODO: cli option to enable this

        let mount_args: Vec<String> = package
            .mounts
            .iter()
            .flat_map(|(from, to)| ["--mount".to_string(), format!("type=bind,source={from},target={to}")])
            .collect();
        args.extend(mount_args);
        args.push(self.distro.image.clone());
        args.push("-c".to_string());
        args.push(commands.join(" && "));

        let log_path = package.output_path.join("build.log");
        let _ = std::fs::remove_file(&log_path);

        Command::container(args)
            .stream_output_to(StreamOutput::Stderr) // TODO: cli option to choose log destination
            .log_to(&log_path)
            .run()
            .map(|_| (package.artefacts(), log_path.clone()))
            .map_err(|code| (code, log_path.clone()))
    }
}
