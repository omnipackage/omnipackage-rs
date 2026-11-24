use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub filename: String,
    pub sha256: String,
}

pub fn find(output: &Path, skip: &str) -> Result<PathBuf> {
    let mut found = vec![];
    for entry in std::fs::read_dir(output)? {
        let path = entry?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if !name.starts_with('.') && name != skip {
            found.push(path);
        }
    }
    match found.as_slice() {
        [one] if one.is_file() => Ok(one.clone()),
        [] => anyhow::bail!("build_script produced nothing in $OMNIPACKAGE_OUTPUT_DIR ({})", output.display()),
        _ => anyhow::bail!(
            "build_script must leave exactly one file in $OMNIPACKAGE_OUTPUT_DIR, found: {}",
            found.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect::<Vec<_>>().join(", ")
        ),
    }
}

pub fn content_type(filename: &str) -> &'static str {
    match Path::new(filename).extension().and_then(|e| e.to_str()) {
        Some("dmg") => "application/x-apple-diskimage",
        Some("zip") => "application/zip",
        Some("gz" | "tgz") => "application/gzip",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_single_artifact_and_ignores_meta_and_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app-1.0.dmg"), b"x").unwrap();
        std::fs::write(dir.path().join(".DS_Store"), b"x").unwrap();
        std::fs::write(dir.path().join("artifact.json"), b"{}").unwrap();
        assert_eq!(find(dir.path(), "artifact.json").unwrap(), dir.path().join("app-1.0.dmg"));
    }

    #[test]
    fn rejects_empty_or_ambiguous_output() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find(dir.path(), "m").unwrap_err().to_string().contains("produced nothing"));
        std::fs::write(dir.path().join("a.dmg"), b"x").unwrap();
        std::fs::write(dir.path().join("b.zip"), b"x").unwrap();
        let err = find(dir.path(), "m").unwrap_err().to_string();
        assert!(err.contains("exactly one") && err.contains("a.dmg") && err.contains("b.zip"), "{err}");
        std::fs::remove_file(dir.path().join("b.zip")).unwrap();
        std::fs::remove_file(dir.path().join("a.dmg")).unwrap();
        std::fs::create_dir(dir.path().join("Hello.app")).unwrap();
        assert!(find(dir.path(), "m").is_err());
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type("a-1.0.dmg"), "application/x-apple-diskimage");
        assert_eq!(content_type("a.zip"), "application/zip");
        assert_eq!(content_type("a-1.0-macos.tar.gz"), "application/gzip");
        assert_eq!(content_type("a.tar.xz"), "application/octet-stream");
    }
}
