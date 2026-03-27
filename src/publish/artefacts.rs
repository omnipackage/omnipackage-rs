use crate::distros::Distro;
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ArtifactMatch {
    pub filename: String,
    pub relative_path: PathBuf,
}

pub fn find_artefacts_in_repository(artefacts: &[PathBuf], work_dir: &Path) -> Result<Vec<ArtifactMatch>, Box<dyn Error>> {
    let mut results = Vec::new();

    for artifact in artefacts {
        let filename = artifact.file_name().ok_or_else(|| format!("artifact has no filename: {}", artifact.display()))?.to_string_lossy();

        let pattern = work_dir.join("**").join(filename.as_ref()).to_string_lossy().into_owned();

        for entry in glob::glob(&pattern)? {
            let full_path = entry?;

            let relative_path = full_path.strip_prefix(work_dir).map(PathBuf::from).unwrap_or_else(|_| full_path.clone());

            results.push(ArtifactMatch {
                filename: filename.to_string(),
                relative_path,
            });
        }
    }

    Ok(results)
}

pub fn find_artefacts_in_build_dir(distro: &'static Distro, build_dir: &Path) -> Vec<PathBuf> {
    let pattern = match distro.package_type.as_str() {
        "rpm" => build_dir.join("RPMS/**/*.rpm"),
        "deb" => build_dir.join("output").join("*.deb"), // NOTE: copy-paste, same logic happens in Package build
        _ => panic!("unknown package type {}", distro.package_type),
    };

    glob::glob(pattern.to_str().unwrap()).unwrap().filter_map(|e| e.ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn finds_nested_artefacts() {
        let dir = tempdir().unwrap();
        let work_dir = dir.path();

        let deep = work_dir.join("sub").join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("foo.txt"), b"").unwrap();

        fs::write(work_dir.join("bar.txt"), b"").unwrap();

        let artefacts = vec![PathBuf::from("/some/original/path/foo.txt"), PathBuf::from("/another/path/bar.txt")];

        let matches = find_artefacts_in_repository(&artefacts, work_dir).unwrap();

        assert_eq!(matches.len(), 2);

        let foo = matches.iter().find(|m| m.filename == "foo.txt").unwrap();
        assert_eq!(foo.relative_path, PathBuf::from("sub/deep/foo.txt"));

        let bar = matches.iter().find(|m| m.filename == "bar.txt").unwrap();
        assert_eq!(bar.relative_path, PathBuf::from("bar.txt"));
    }

    #[test]
    fn no_match_returns_empty() {
        let dir = tempdir().unwrap();
        let artefacts = vec![PathBuf::from("/path/to/missing.txt")];
        let matches = find_artefacts_in_repository(&artefacts, dir.path()).unwrap();
        assert!(matches.is_empty());
    }
}
