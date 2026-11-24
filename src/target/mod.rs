use crate::LoggingArgs;
use crate::config::{Build, Repository};
use crate::distros::Distros;
use crate::release::JobSetup;
use anyhow::Result;
use std::path::PathBuf;

mod linux;
mod mac;
pub mod source_archive;

pub struct TargetContext<'a> {
    pub job: &'a JobSetup,
    pub logging: &'a LoggingArgs,
    pub fail_fast: bool,
}

// Err aborts the whole command; Ok(false) is a failure of this target only (already logged, subject to --fail-fast)
pub trait Target {
    fn build(&mut self, ctx: &TargetContext) -> Result<bool>;
    fn publish(&mut self, ctx: &TargetContext, repository: &Repository, custom_install_page: Option<PathBuf>) -> Result<bool>;
    fn release(&mut self, ctx: &TargetContext, repository: &Repository, custom_install_page: Option<PathBuf>) -> Result<bool>;
}

pub fn make(ctx: &TargetContext, build: Build) -> Result<Box<dyn Target>> {
    if build.distro == mac::ID {
        return Ok(Box::new(mac::MacTarget::new(build)?));
    }
    Ok(Box::new(linux::LinuxTarget::new(ctx, build)?))
}

pub fn is_supported(id: &str) -> bool {
    host_os(id).is_some()
}

pub fn is_native(build: &Build) -> bool {
    host_os(&build.distro) == Some(std::env::consts::OS) || (build.distro == mac::ID && mac::MacTarget::is_containerized(build))
}

fn host_os(id: &str) -> Option<&'static str> {
    if id == mac::ID {
        Some("macos")
    } else if is_container(id) {
        Some("linux")
    } else {
        None
    }
}

pub fn is_container(id: &str) -> bool {
    Distros::get().contains(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HomebrewConfig;

    fn build(distro: &str, image: Option<&str>) -> Build {
        Build {
            distro: distro.to_string(),
            package_name: "hello".to_string(),
            maintainer: "m".to_string(),
            homepage: "h".to_string(),
            description: "d".to_string(),
            build_dependencies: vec![],
            runtime_dependencies: vec![],
            before_build_script: None,
            rpm: None,
            deb: None,
            pacman: None,
            homebrew: Some(HomebrewConfig {
                formula_template: Some("f".to_string()),
                cask_template: None,
                build_script: Some("s".to_string()),
                image: image.map(str::to_string),
            }),
            rest: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn registry_knows_container_and_host_targets() {
        assert!(is_supported("debian_13") && is_container("debian_13"));
        assert!(is_supported("macos") && !is_container("macos"));
        assert!(!is_supported("nonexistent_99"));
        assert_eq!(is_native(&build("debian_13", None)), !cfg!(target_os = "macos"));
        assert_eq!(is_native(&build("macos", None)), cfg!(target_os = "macos"));
        assert!(is_native(&build("macos", Some("golang:1.25"))));
        assert!(!is_native(&build("nonexistent_99", None)));
    }
}
