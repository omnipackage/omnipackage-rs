use super::artefacts;
use super::cloudflare::CloudflareApi;
use super::locking::{self, DerivedFile};
use super::s3::S3;
use crate::config::{Repository, RepositoryProvider, S3Config};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum Store {
    S3 { config: Box<S3Config>, root: String },
    LocalFs { root: PathBuf },
}

impl Store {
    pub fn new(repository: &Repository) -> Self {
        match repository.provider {
            RepositoryProvider::S3 => {
                let config = Box::new(repository.s3().clone());
                let root = config.path_in_bucket.clone().unwrap_or_default();
                Store::S3 { config, root }
            }
            RepositoryProvider::LocalFs => Store::LocalFs {
                root: repository.localfs().repository_path(),
            },
        }
    }

    pub fn scoped(&self, sub: &str) -> Self {
        match self {
            Store::S3 { config, root } => Store::S3 {
                config: config.clone(),
                root: join_key(root, sub),
            },
            Store::LocalFs { root } => Store::LocalFs { root: root.join(sub) },
        }
    }

    pub fn public_url(&self, key: &str) -> String {
        match self {
            Store::S3 { config, root } => format!("{}/{}", config.base_url(), join_key(root, key)),
            Store::LocalFs { root } if key.is_empty() => root.to_string_lossy().into_owned(),
            Store::LocalFs { root } => root.join(key).to_string_lossy().into_owned(),
        }
    }

    pub fn put(&self, key: &str, data: Vec<u8>, content_type: Option<&str>) -> Result<()> {
        match self {
            Store::S3 { config, root } => S3::new(config, root.clone()).upload_file(key, data, content_type),
            Store::LocalFs { root } => write_file(&root.join(key), &data),
        }
    }

    pub fn put_file(&self, key: &str, path: &Path, content_type: Option<&str>) -> Result<()> {
        let data = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        self.put(key, data, content_type)
    }

    pub fn download_dir(&self, to: &Path) -> Result<()> {
        std::fs::create_dir_all(to)?;
        match self {
            Store::S3 { config, root } => S3::new(config, root.clone()).download_all(to),
            Store::LocalFs { root } => {
                if root.exists() {
                    artefacts::copy_dir_recursive(root, to, &HashSet::new())?;
                }
                Ok(())
            }
        }
    }

    pub fn commit_anchored<F>(&self, anchor_key: &str, anchor_content_type: &str, mut build: F) -> Result<()>
    where
        F: FnMut(&[u8]) -> Result<(Vec<u8>, Vec<DerivedFile>)>,
    {
        match self {
            Store::S3 { config, root } => locking::commit_anchored(&S3::new(config, root.clone()), anchor_key, anchor_content_type, build),
            Store::LocalFs { root } => {
                let anchor_path = root.join(anchor_key);
                let existing = std::fs::read(&anchor_path).unwrap_or_default();
                let (anchor_body, derived) = build(&existing)?;
                for (key, _, body) in &derived {
                    write_file(&root.join(key), body)?;
                }
                write_file(&anchor_path, &anchor_body)
            }
        }
    }

    pub fn purge_cdn(&self) -> Result<()> {
        let Store::S3 { config, .. } = self else {
            return Ok(());
        };
        let (Some(zone_id), Some(api_token)) = (&config.cloudflare_zone_id, &config.cloudflare_api_token) else {
            return Ok(());
        };
        let url = self.public_url("");
        let prefix = url.trim_start_matches("https://").trim_start_matches("http://");
        CloudflareApi::new(zone_id.clone(), api_token.clone()).purge_by_prefix(prefix)
    }
}

fn join_key(root: &str, key: &str) -> String {
    match (root.is_empty(), key.is_empty()) {
        (true, _) => key.to_string(),
        (_, true) => root.to_string(),
        _ => format!("{}/{}", root.trim_end_matches('/'), key.trim_start_matches('/')),
    }
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("cannot create dir {}", parent.display()))?;
    }
    std::fs::write(path, data).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s3_store(path_in_bucket: Option<&str>) -> Store {
        let config = S3Config {
            bucket: "b".to_string(),
            path_in_bucket: path_in_bucket.map(str::to_string),
            bucket_public_url: Some("https://cdn.example.com/".to_string()),
            endpoint: "https://s3.example.com".to_string(),
            access_key_id: "k".to_string(),
            secret_access_key: "s".to_string(),
            region: None,
            force_path_style: true,
            cloudflare_zone_id: None,
            cloudflare_api_token: None,
        };
        let root = config.path_in_bucket.clone().unwrap_or_default();
        Store::S3 { config: Box::new(config), root }
    }

    #[test]
    fn s3_public_url_matches_base_bucket_url() {
        let store = s3_store(Some("proj/stable"));
        assert_eq!(store.public_url(""), "https://cdn.example.com/proj/stable");
        assert_eq!(store.scoped("macos").public_url("tap.git"), "https://cdn.example.com/proj/stable/macos/tap.git");
        assert_eq!(s3_store(None).scoped("macos").public_url("src/a.tar.gz"), "https://cdn.example.com/macos/src/a.tar.gz");
    }

    #[test]
    fn localfs_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::LocalFs { root: dir.path().to_path_buf() }.scoped("macos");

        store.put("bottles/a.tar.gz", b"bottle".to_vec(), None).unwrap();
        store
            .commit_anchored("tap.git/info/refs", "text/plain", |existing| {
                assert!(existing.is_empty());
                Ok((b"refs".to_vec(), vec![("tap.git/objects/ab/cd".to_string(), "application/octet-stream", b"obj".to_vec())]))
            })
            .unwrap();

        let out = dir.path().join("out");
        store.download_dir(&out).unwrap();
        assert_eq!(std::fs::read(out.join("bottles/a.tar.gz")).unwrap(), b"bottle");
        assert_eq!(std::fs::read(out.join("tap.git/info/refs")).unwrap(), b"refs");
        assert_eq!(std::fs::read(out.join("tap.git/objects/ab/cd")).unwrap(), b"obj");
        assert_eq!(store.public_url("tap.git"), dir.path().join("macos/tap.git").to_string_lossy());

        store.commit_anchored("tap.git/info/refs", "text/plain", |existing| Ok((existing.to_vec(), vec![]))).unwrap();
    }

    #[test]
    fn download_dir_of_missing_localfs_root_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::LocalFs { root: dir.path().join("nope") };
        let out = dir.path().join("out");
        store.download_dir(&out).unwrap();
        assert!(std::fs::read_dir(&out).unwrap().next().is_none());
    }
}
