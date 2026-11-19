use super::repo_files;
use super::store::Store;
use crate::config::{Repository, RepositoryProvider};
use anyhow::Result;

pub const INSTALL_PAGE_NAME: &str = "install.html";
pub const INSTALL_JSON_NAME: &str = "install.json";
pub const INSTALL_SCRIPT_NAME: &str = "install.sh";
pub const BADGE_NAME: &str = "badge.svg";

#[derive(Debug, Clone)]
pub struct InstallPageBadge {
    pub page_url: String,
    pub badge_md: String,
}

pub fn publish_entry(repository: &Repository, entry: repo_files::Repository, custom_template: Option<String>, arch: Option<&str>) -> Result<InstallPageBadge> {
    let store = Store::new(repository);
    let entries = enrich_for_json(&vec![entry], arch, &repository.package_name);
    let base_url = store.public_url("");

    store.commit_anchored(INSTALL_PAGE_NAME, "text/html", |existing_html| {
        let existing = repo_files::html::parse(&String::from_utf8_lossy(existing_html)).unwrap_or_default();
        let merged = repo_files::upsert_from(existing, &entries);
        let html = repo_files::html::render_page(&merged, repository, custom_template.clone())?;
        Ok((html.into_bytes(), vec![]))
    })?;

    store.commit_anchored(INSTALL_JSON_NAME, "application/json", |existing_json| {
        let existing = repo_files::json::parse(&String::from_utf8_lossy(existing_json));
        let merged = repo_files::upsert_from(existing, &entries);
        let json = repo_files::json::to_json(&merged)?;
        let sh = repo_files::sh::render(&merged, &repository.package_name, &base_url, &repo_arch(arch, &merged))?;
        let badge = repo_files::badge::render(&merged, repository)?;
        Ok((
            json.into_bytes(),
            vec![
                (INSTALL_SCRIPT_NAME.to_string(), "text/x-shellscript", sh.into_bytes()),
                (BADGE_NAME.to_string(), "image/svg+xml", badge.into_bytes()),
            ],
        ))
    })?;

    Ok(match repository.provider {
        RepositoryProvider::S3 => {
            let page_url = install_page_url(repository).unwrap_or_default();
            let badge_url = format!("{}/{}", repository.s3().base_bucket_url(), BADGE_NAME);
            let badge_md = format!("[![OmniPackage repositories badge]({badge_url})]({page_url})");
            InstallPageBadge { page_url, badge_md }
        }
        RepositoryProvider::LocalFs => InstallPageBadge {
            page_url: store.public_url(INSTALL_PAGE_NAME),
            badge_md: "".to_string(),
        },
    })
}

pub fn install_page_url(repository: &Repository) -> Option<String> {
    match repository.provider {
        RepositoryProvider::S3 => Some(format!("{}/{}", repository.s3().base_bucket_url(), INSTALL_PAGE_NAME)),
        _ => None,
    }
}

fn repo_arch(arch: Option<&str>, merged: &repo_files::Repositories) -> String {
    arch.map(str::to_string)
        .or_else(|| merged.iter().find_map(|e| e.get("arch").cloned()))
        .unwrap_or_else(|| std::env::consts::ARCH.to_string())
}

fn enrich_for_json(entries: &repo_files::Repositories, arch: Option<&str>, package_name: &str) -> repo_files::Repositories {
    entries
        .iter()
        .map(|e| {
            let mut m = e.clone();
            if let Some(arch) = arch {
                m.insert("arch".to_string(), arch.to_string());
            }
            m.insert("package_name".to_string(), package_name.to_string());
            m
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LocalFsConfig;
    use std::collections::HashMap;

    fn localfs_repository(path: &std::path::Path) -> Repository {
        Repository {
            name: "Local".to_string(),
            provider: RepositoryProvider::LocalFs,
            localfs: Some(LocalFsConfig {
                path: path.to_string_lossy().into_owned(),
            }),
            s3: None,
            gpg_private_key_base64: String::new(),
            package_name: "hello".to_string(),
            retain_packages: 0,
            rest: HashMap::new(),
        }
    }

    fn entry(pairs: &[(&str, &str)]) -> repo_files::Repository {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn linux_and_macos_entries_share_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let repo = localfs_repository(dir.path());

        let linux = entry(&[
            ("distro_id", "debian_13"),
            ("distro_name", "Debian 13"),
            ("distro_family", "Debian"),
            ("install_steps", "apt-get install hello"),
            ("gpg_key", "KEY"),
            ("download_url", "file:///r/debian_13/hello.deb"),
            ("package_type", "deb"),
        ]);
        publish_entry(&repo, linux, None, Some("x86_64")).unwrap();

        let mac = entry(&[
            ("distro_id", "macos"),
            ("distro_name", "macOS (Homebrew)"),
            ("distro_family", "macOS"),
            ("install_steps", "brew tap omnipackage/hello /r/macos/tap.git\nbrew install omnipackage/hello/hello"),
            ("download_url", "file:///r/macos/bottles/hello-1.0.arm64_sequoia.bottle.tar.gz"),
            ("package_type", "homebrew"),
            ("install_as_root", "false"),
        ]);
        let badge = publish_entry(&repo, mac, None, None).unwrap();
        assert_eq!(badge.page_url, dir.path().join(INSTALL_PAGE_NAME).to_string_lossy());

        let json = repo_files::json::parse(&std::fs::read_to_string(dir.path().join(INSTALL_JSON_NAME)).unwrap());
        assert_eq!(json.len(), 2);
        let mac_json = json.iter().find(|e| e["distro_id"] == "macos").unwrap();
        assert!(!mac_json.contains_key("arch"));
        assert_eq!(mac_json["package_name"], "hello");

        let html = repo_files::html::parse(&std::fs::read_to_string(dir.path().join(INSTALL_PAGE_NAME)).unwrap()).unwrap();
        assert_eq!(html.len(), 2);

        let sh = std::fs::read_to_string(dir.path().join(INSTALL_SCRIPT_NAME)).unwrap();
        assert!(sh.contains("REPO_ARCH='x86_64'"), "linux repo arch must survive a macos publish");
        assert!(sh.contains("  macos)"));

        let badge_svg = std::fs::read_to_string(dir.path().join(BADGE_NAME)).unwrap();
        assert!(badge_svg.contains("1 BREW"));
    }
}
