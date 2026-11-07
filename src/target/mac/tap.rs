use crate::shell::Command;
use crate::target::source_archive::sha256_file;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

pub type Snapshot = HashMap<String, String>;

fn git() -> Command {
    Command::new("git").args([
        "-c",
        "gc.auto=0",
        "-c",
        "init.defaultBranch=main",
        "-c",
        "user.name=omnipackage",
        "-c",
        "user.email=omnipackage@localhost",
        "-c",
        "commit.gpgsign=false",
    ])
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn init_bare(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    git().args(["init", "-q", "--bare"]).arg(path_arg(dir)).run().context("git init --bare failed")
}

pub fn clone(bare: &Path, worktree: &Path) -> Result<()> {
    git().args(["clone", "-q"]).arg(path_arg(bare)).arg(path_arg(worktree)).run().context("git clone failed")
}

pub fn commit_all(worktree: &Path, message: &str) -> Result<bool> {
    git().arg("-C").arg(path_arg(worktree)).args(["add", "-A"]).run().context("git add failed")?;
    let status = git().arg("-C").arg(path_arg(worktree)).args(["status", "--porcelain"]).capture().context("git status failed")?;
    if status.trim().is_empty() {
        return Ok(false);
    }
    git().arg("-C").arg(path_arg(worktree)).args(["commit", "-q", "-m", message]).run().context("git commit failed")?;
    Ok(true)
}

pub fn push(worktree: &Path) -> Result<()> {
    git()
        .arg("-C")
        .arg(path_arg(worktree))
        .args(["push", "-q", "origin", "HEAD:refs/heads/main"])
        .run()
        .context("git push failed")
}

pub fn update_server_info(bare: &Path) -> Result<()> {
    git().arg("-C").arg(path_arg(bare)).arg("update-server-info").run().context("git update-server-info failed")
}

pub fn snapshot(dir: &Path) -> Result<Snapshot> {
    let mut files = Snapshot::new();
    if dir.exists() {
        walk(dir, dir, &mut files)?;
    }
    Ok(files)
}

fn walk(root: &Path, dir: &Path, files: &mut Snapshot) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            walk(root, &path, files)?;
        } else {
            let rel = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
            files.insert(rel, sha256_file(&path)?);
        }
    }
    Ok(())
}

// dumb-HTTP clients start from info/refs, so everything it can point to must be uploaded first; info/refs itself is the caller's anchor
pub fn changed_files(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut changed: Vec<String> = after
        .iter()
        .filter(|(path, sha)| before.get(*path) != Some(*sha))
        .map(|(path, _)| path.clone())
        .filter(|path| path != "info/refs" && !path.starts_with("hooks/"))
        .collect();
    changed.sort_by_key(|path| (upload_rank(path), path.clone()));
    changed
}

fn upload_rank(path: &str) -> u8 {
    if path.starts_with("objects/info/") {
        2
    } else if path.starts_with("objects/") {
        0
    } else if path.starts_with("refs/") || path == "packed-refs" {
        1
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_formula(worktree: &Path, body: &str) -> bool {
        std::fs::create_dir_all(worktree.join("Formula")).unwrap();
        std::fs::write(worktree.join("Formula/hello.rb"), body).unwrap();
        commit_all(worktree, "hello").unwrap()
    }

    #[test]
    fn publishes_only_new_objects_in_dumb_http_order() {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("tap.git");
        let worktree = dir.path().join("work");

        init_bare(&bare).unwrap();
        let empty = snapshot(&bare).unwrap();
        clone(&bare, &worktree).unwrap();
        assert!(commit_formula(&worktree, "class Hello < Formula\nend\n"));
        push(&worktree).unwrap();
        update_server_info(&bare).unwrap();

        let first = snapshot(&bare).unwrap();
        assert!(first.contains_key("info/refs"));
        assert!(first.contains_key("refs/heads/main"));
        let changed = changed_files(&empty, &first);
        assert!(!changed.contains(&"info/refs".to_string()));
        assert!(changed.iter().all(|p| !p.starts_with("hooks/")));
        let first_ref = changed.iter().position(|p| p.starts_with("refs/")).unwrap();
        let last_object = changed.iter().rposition(|p| p.starts_with("objects/") && !p.starts_with("objects/info/")).unwrap();
        assert!(last_object < first_ref, "objects must precede refs: {changed:?}");

        assert!(!commit_formula(&worktree, "class Hello < Formula\nend\n"), "unchanged formula must not commit");
        assert!(commit_formula(&worktree, "class Hello < Formula\n  # v2\nend\n"));
        push(&worktree).unwrap();
        update_server_info(&bare).unwrap();
        let second = snapshot(&bare).unwrap();
        let changed = changed_files(&first, &second);
        assert!(changed.contains(&"refs/heads/main".to_string()));
        assert!(changed.iter().all(|p| p.starts_with("objects/") || p.starts_with("refs/") || p == "packed-refs"), "{changed:?}");
        assert_eq!(
            changed.iter().filter(|p| p.starts_with("objects/") && !p.starts_with("objects/info/")).count(),
            4,
            "commit, root tree, Formula tree, blob: {changed:?}"
        );
    }
}
