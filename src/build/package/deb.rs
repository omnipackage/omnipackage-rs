use crate::build::BuildContext;
use crate::build::job_variables::JobVariables;
use crate::build::package::Package;
use crate::config::Build;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

impl BuildContext {
    pub fn setup_deb(&self) -> Package {
        Package {
            mounts: HashMap::new(),
            commands: Vec::new(),
            source_path: self.source_path.clone(),
            output_path: "ololo".into(),
        }
    }
}
