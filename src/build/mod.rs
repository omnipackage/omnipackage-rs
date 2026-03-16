use crate::config::{Build, Config};
use crate::distros::{Distro, Distros};
use std::path::PathBuf;
use std::time::Instant;

mod extract_version;
mod job_variables;

use job_variables::JobVariables;

pub fn run(distro_ids: Vec<String>, path: PathBuf) {
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
        }
        .run();
    }
}

pub struct BuildContext {
    pub distro: &'static Distro,
    pub path: PathBuf,
    pub config: Build,
    pub job_variables: JobVariables,
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

        crate::logger::info(format!("successfully finished build for {} in {:.1}s", self.distro.id, started_at.elapsed().as_secs_f32()));
    }
}
