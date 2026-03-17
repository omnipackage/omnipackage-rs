use crate::build::job_variables::JobVariables;
use crate::build::package::{PackageInput, PackageOutput};
use crate::config::Build;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

impl PackageInput {
    pub fn setup_deb(&self) -> PackageOutput {
        PackageOutput {
            mounts: HashMap::new(),
            commands: Vec::new(),
            source_path: self.source_path.clone(),
            output_path: "ololo".into(),
        }
    }
}
