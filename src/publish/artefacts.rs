use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ArtifactMatch {
    pub filename: String,
    pub relative_path: PathBuf,
}

pub fn find_artefacts_in_repository(artefacts: &[PathBuf], work_dir: &Path) -> Result<Vec<ArtifactMatch>, Box<dyn std::error::Error>> {
    let mut results = Vec::new();

    for artifact in artefacts {
        // Extract just the filename component (e.g. "foo.tar.gz")
        let filename = artifact.file_name().ok_or_else(|| format!("artifact has no filename: {}", artifact.display()))?.to_string_lossy();

        // Build a recursive glob: <work_dir>/**/filename
        let pattern = work_dir.join("**").join(filename.as_ref()).to_string_lossy().into_owned();

        for entry in glob::glob(&pattern)? {
            let full_path = entry?;

            // Strip work_dir prefix to get the relative path
            let relative_path = full_path.strip_prefix(work_dir).map(PathBuf::from).unwrap_or_else(|_| full_path.clone());

            results.push(ArtifactMatch {
                filename: filename.to_string(),
                relative_path,
            });
        }
    }

    Ok(results)
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

        // Create work_dir/sub/deep/foo.txt
        let deep = work_dir.join("sub").join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("foo.txt"), b"").unwrap();

        // Also work_dir/bar.txt at root level
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
