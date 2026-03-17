use crate::config::{Build, Config};
use crate::distros::{Distro, Distros};
use std::path::PathBuf;
use std::time::Instant;

mod extract_version;
mod job_variables;
pub mod package;

use job_variables::JobVariables;
use package::{PackageInput, PackageOutput};

pub fn run(distro_ids: Vec<String>, path: PathBuf, build_dir: PathBuf) {
    let config = Config::load(&path.join(".omnipackage/config.yml"));

    let version = extract_version::extract_version(&path, &config.extract_version);
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
            path: path.clone(),
            config: build.clone(),
            job_variables: job_variables.clone(),
            build_dir: build_dir.clone(),
        }
        .run();
    }
}

pub struct BuildContext {
    pub distro: &'static Distro,
    pub path: PathBuf,
    pub config: Build,
    pub job_variables: JobVariables,
    pub build_dir: PathBuf,
}

impl BuildContext {
    pub fn run(&self) {
        crate::logger::info(format!(
            "starting build for {} at {}, variables: {}",
            self.distro.id,
            self.path.display(),
            self.job_variables
        ));
        let started_at = Instant::now();

        let package_input = PackageInput {
            build_config: self.config.clone(),
            build_dir: self.build_dir.clone(),
            job_variables: self.job_variables.clone(),
            source_path: self.path.clone(),
            distro: self.distro,
        };

        let package_output = match self.distro.package_type.as_str() {
            "rpm" => package_input.setup_rpm(),
            "deb" => package_input.setup_deb(),
            _ => panic!("unknown package type {}", self.distro.package_type),
        };

        crate::logger::info(format!("successfully finished build for {} in {:.1}s", self.distro.id, started_at.elapsed().as_secs_f32()));
    }
}
