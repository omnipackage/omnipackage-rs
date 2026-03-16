use crate::build::job_variables::JobVariables;
use crate::build::package::Package;
use crate::config::Build;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct Deb {
    pub build_config: Build,
    pub build_dir: PathBuf,
    pub job_variables: JobVariables,
    pub source_path: String,
    pub distro: &'static Distro,
}

impl Package for Deb {
    fn setup(&self) {}

    fn output_path(&self) -> PathBuf {
        "123".into()
    }

    fn mounts(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn commands(&self) -> Vec<String> {
        Vec::new()
    }
}
