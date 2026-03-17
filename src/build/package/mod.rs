use std::collections::HashMap;
use std::path::PathBuf;

pub mod deb;
pub mod rpm;
pub mod template;

pub struct Package {
    pub mounts: HashMap<String, String>,
    pub commands: Vec<String>,
    pub output_path: PathBuf,
}
