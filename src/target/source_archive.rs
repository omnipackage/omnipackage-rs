use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

// ignore_source_files semantics (same as the rsync excludes in containers): "name" and "*.ext" match a basename at any depth, "/name" only at the root
pub fn stage(source_dir: &Path, ignore: &[String], dest: &Path) -> Result<()> {
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::create_dir_all(dest)?;
    copy_filtered(source_dir, dest, Path::new(""), ignore)
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn is_ignored(rel: &Path, ignore: &[String]) -> bool {
    let name = rel.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let rel_str = rel.to_string_lossy();
    ignore.iter().any(|pattern| match pattern.strip_prefix('/') {
        Some(anchored) => glob::Pattern::new(anchored.trim_end_matches('/')).is_ok_and(|p| p.matches(&rel_str)),
        None => glob::Pattern::new(pattern.trim_end_matches('/')).is_ok_and(|p| p.matches(&name)),
    })
}

fn copy_filtered(src_root: &Path, dst_root: &Path, rel: &Path, ignore: &[String]) -> Result<()> {
    for entry in std::fs::read_dir(src_root.join(rel)).with_context(|| format!("cannot read {}", src_root.join(rel).display()))? {
        let entry = entry?;
        let child_rel = rel.join(entry.file_name());
        if is_ignored(&child_rel, ignore) {
            continue;
        }
        let src = src_root.join(&child_rel);
        let dst = dst_root.join(&child_rel);
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&src)?, &dst)?;
        } else if file_type.is_dir() {
            std::fs::create_dir_all(&dst)?;
            copy_filtered(src_root, dst_root, &child_rel, ignore)?;
        } else {
            std::fs::copy(&src, &dst).with_context(|| format!("cannot copy {}", src.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(root: &Path) {
        for (path, body) in [
            ("main.c", "int main(){}"),
            ("build.log", "x"),
            ("target/out", "x"),
            ("src/target/keep.rs", "x"),
            ("src/.git/HEAD", "x"),
            (".env", "SECRET=1"),
        ] {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
    }

    #[test]
    fn excludes_follow_ignore_source_files_semantics() {
        let ignore: Vec<String> = [".git", ".env", "*.log", "/target"].iter().map(|s| s.to_string()).collect();
        assert!(is_ignored(Path::new("src/.git"), &ignore));
        assert!(is_ignored(Path::new("a/b/build.log"), &ignore));
        assert!(is_ignored(Path::new("target"), &ignore));
        assert!(!is_ignored(Path::new("src/target"), &ignore));
        assert!(!is_ignored(Path::new("main.c"), &ignore));
    }

    #[test]
    fn stages_filtered_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("project");
        tree(&source);
        let ignore: Vec<String> = [".git", ".env", "*.log", "/target"].iter().map(|s| s.to_string()).collect();

        let dest = dir.path().join("stage/hello-1.0");
        stage(&source, &ignore, &dest).unwrap();
        assert!(dest.join("main.c").exists());
        assert!(dest.join("src/target/keep.rs").exists());
        assert!(!dest.join("build.log").exists());
        assert!(!dest.join(".env").exists());
        assert!(!dest.join("src/.git").exists());
        assert!(!dest.join("target").exists());
    }

    #[test]
    fn sha256_of_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
