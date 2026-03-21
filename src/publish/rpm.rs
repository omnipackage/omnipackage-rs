use crate::publish::PublishContext;
use std::path::{Path, PathBuf};

impl PublishContext {
    pub fn setup_rpm_repo(&self, dir: &Path) -> Result<(), String> {
        Ok(())
    }
}
