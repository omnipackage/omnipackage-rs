use crate::build::job_variables::JobVariables;
use crate::config::Build;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

mod deb;
mod rpm;

pub trait Package {
    fn setup(&self);
    fn output_path(&self) -> PathBuf;
    fn mounts(&self) -> HashMap<String, String>;
    fn commands(&self) -> Vec<String>;
}
