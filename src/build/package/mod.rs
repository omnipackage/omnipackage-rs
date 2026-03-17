use crate::build::job_variables::JobVariables;
use crate::config::Build;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

mod deb;
mod rpm;
pub mod template;

pub struct PackageInput {
    pub build_config: Build,
    pub build_dir: PathBuf,
    pub job_variables: JobVariables,
    pub source_path: PathBuf,
    pub distro: &'static Distro,
}

pub struct PackageOutput {
    pub mounts: HashMap<String, String>,
    pub commands: Vec<String>,
    pub source_path: PathBuf,
    pub output_path: PathBuf,
}
