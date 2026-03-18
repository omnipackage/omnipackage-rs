use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

mod deb;
mod rpm;
pub mod template;

pub struct Package {
    pub distro: &'static Distro,
    pub mounts: HashMap<String, String>,
    pub commands: Vec<String>,
    pub output_path: PathBuf,
}

impl Package {
    pub fn artefacts(&self) -> Vec<PathBuf> {
        let pattern = match self.distro.package_type.as_str() {
            "rpm" => self.output_path.join("RPMS/**/*.rpm"),
            "deb" => self.output_path.join("*.deb"),
            _ => panic!("unknown package type {}", self.distro.package_type),
        };

        glob::glob(pattern.to_str().unwrap()).unwrap().filter_map(|e| e.ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_package(output_path: PathBuf, package_type: &str) -> Package {
        Package {
            mounts: Default::default(),
            commands: vec![],
            output_path,
            distro: Box::leak(Box::new(Distro {
                id: "test".to_string(),
                name: "Test".to_string(),
                image: "test:latest".to_string(),
                arch: "x86_64".to_string(),
                package_type: package_type.to_string(),
                setup: vec![],
                setup_repo: vec![],
                install_steps: vec![],
                image_info_url: None,
                deprecated: None,
            })),
        }
    }

    #[test]
    fn test_artefacts_rpm() {
        let dir = tempfile::tempdir().unwrap();
        let rpms_dir = dir.path().join("RPMS/x86_64");
        std::fs::create_dir_all(&rpms_dir).unwrap();
        std::fs::write(rpms_dir.join("myapp-1.0.rpm"), "").unwrap();
        std::fs::write(rpms_dir.join("myapp-debuginfo-1.0.rpm"), "").unwrap();

        let package = make_package(dir.path().to_path_buf(), "rpm");
        let artefacts = package.artefacts();

        assert_eq!(artefacts.len(), 2);
        assert!(artefacts.iter().all(|p| p.extension().unwrap() == "rpm"));
    }

    #[test]
    fn test_artefacts_deb() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("myapp_1.0_amd64.deb"), "").unwrap();
        std::fs::write(dir.path().join("myapp-dbgsym_1.0_amd64.deb"), "").unwrap();

        let package = make_package(dir.path().to_path_buf(), "deb");
        let artefacts = package.artefacts();

        assert_eq!(artefacts.len(), 2);
        assert!(artefacts.iter().all(|p| p.extension().unwrap() == "deb"));
    }

    #[test]
    fn test_artefacts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let package = make_package(dir.path().to_path_buf(), "rpm");
        assert!(package.artefacts().is_empty());
    }
}
