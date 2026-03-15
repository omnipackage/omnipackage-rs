use crate::config::Config;
use crate::distros::{Distro, Distros};
use std::path::PathBuf;
use std::time::Instant;

mod extract_version;

pub fn run(distro_ids: Vec<String>, path: PathBuf) {
    let config = Config::load(&path.join(".omnipackage/config.yml"));

    let version = extract_version::extract_version(&path, &config);
    println!("version: {}", version);

    let all = Distros::get();
    let distros_to_build: Vec<&Distro> = if distro_ids.is_empty() {
        all.distros.iter().collect()
    } else {
        distro_ids.iter().map(|id| all.by_id(id)).collect()
    };

    for distro in distros_to_build {
        BuildContext {
            distro: distro,
            path: path.clone(),
            config: config.clone(),
        }
        .run();
    }
}

pub struct BuildContext {
    pub distro: &'static Distro,
    pub path: PathBuf,
    pub config: Config,
}

impl BuildContext {
    pub fn run(&self) {
        crate::logger::info(format!("starting build for {} at {}", self.distro.id, self.path.display()));
        let started_at = Instant::now();

        crate::logger::info(format!("successfully finished build for {} in {:.1}s", self.distro.id, started_at.elapsed().as_secs_f32()));
    }
}
