use crate::build::job_variables::JobVariables;
use crate::config::Build;
use crate::build::BuildContext;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

mod deb;
mod rpm;
pub mod template;

pub struct PackageInput {
    pub build_context: BuildContext,
}

pub struct PackageOutput {
    pub mounts: HashMap<String, String>,
    pub commands: Vec<String>,
    pub source_path: PathBuf,
    pub output_path: PathBuf,
}
