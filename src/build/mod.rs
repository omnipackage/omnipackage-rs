use crate::config::{Build, Config};
use crate::distros::{Distro, Distros};
use crate::shell::{Command, StreamOutput};
use std::path::PathBuf;
use std::result::Result;
use std::time::Instant;

mod extract_version;
mod job_variables;
pub mod package;

use job_variables::JobVariables;
use package::Package;

pub fn run(distro_ids: Vec<String>, source_path: PathBuf, build_dir: PathBuf) {
    let config = Config::load(&source_path.join(".omnipackage/config.yml"));

    let version = extract_version::extract_version(&source_path, &config.extract_version);
    let job_variables = JobVariables::build(version);

    for build in &config.builds {
        if !Distros::get().contains(&build.distro) {
            continue;
        }
        if !distro_ids.is_empty() && !distro_ids.contains(&build.distro) {
            continue;
        };

        BuildContext {
            distro: Distros::get().by_id(&build.distro),
            source_path: source_path.clone(),
            config: build.clone(),
            job_variables: job_variables.clone(),
            build_dir: build_dir.clone(),
        }
        .run();
    }
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
    pub fn run(&self) {
        crate::logger::info(format!(
            "starting build for {} at {}, variables: {}",
            self.distro.id,
            self.source_path.display(),
            self.job_variables
        ));
        let started_at = Instant::now();

        let package = self.setup_package();
        match self.execute(&package) {
            Ok(()) => {
                crate::logger::info(format!("successfully finished build for {} in {:.1}s", self.distro.id, started_at.elapsed().as_secs_f32()));
            }
            Err((code, log_path)) => {
                crate::logger::error(format!(
                    "failed build for {} in {:.1}s, log: {}",
                    self.distro.id,
                    started_at.elapsed().as_secs_f32(),
                    log_path.display()
                ));
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

    fn execute(&self, package: &Package) -> Result<(), (i32, std::path::PathBuf)> {
        let mut args = vec!["run".to_string(), "--rm".to_string(), "--entrypoint".to_string(), "/bin/sh".to_string()];
        let mount_args: Vec<String> = package
            .mounts
            .iter()
            .flat_map(|(from, to)| ["--mount".to_string(), format!("type=bind,source={from},target={to}")])
            .collect();
        args.extend(mount_args);
        args.push(self.distro.image.clone());
        args.push("-c".to_string());
        args.push(package.commands.join(" && "));

        let log_path = package.output_path.join("build.log");
        std::fs::remove_file(&log_path);

        Command::container(args)
            .stream_output_to(StreamOutput::Stderr)
            .log_to(&log_path)
            .run()
            .map_err(|code| (code, log_path))
    }
}
