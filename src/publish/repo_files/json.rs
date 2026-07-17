use super::{Repositories, sorted_by_distro_order, upsert_repository};

pub fn upsert(existing_json: &str, entries: &Repositories) -> Result<(String, Repositories), anyhow::Error> {
    let trimmed = existing_json.trim();
    let mut repos: Repositories = if trimmed.is_empty() { Vec::new() } else { serde_json::from_str(trimmed).unwrap_or_default() };

    for entry in entries {
        upsert_repository(&mut repos, entry.clone());
    }

    let merged = sorted_by_distro_order(&repos);
    let json = serde_json::to_string_pretty(&merged)?;
    Ok((json, merged))
}

#[cfg(test)]
mod tests {
    use super::super::{Repositories, Repository};
    use super::*;

    fn entry(id: &str, extra: &[(&str, &str)]) -> Repository {
        let mut m = Repository::from([("distro_id".to_string(), id.to_string())]);
        for (k, v) in extra {
            m.insert((*k).to_string(), (*v).to_string());
        }
        m
    }

    #[test]
    fn test_upsert_empty_start_produces_array() {
        let (json, merged) = upsert("", &vec![entry("ubuntu_24.04", &[("arch", "x86_64"), ("package_name", "omnipackage")])]).unwrap();
        assert!(json.trim_start().starts_with('['), "not a bare array: {json}");
        let parsed: Repositories = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["arch"], "x86_64");
        assert_eq!(parsed[0]["package_name"], "omnipackage");
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn test_upsert_merges_two_distros() {
        let (json, _) = upsert("", &vec![entry("fedora_42", &[])]).unwrap();
        let (json, merged) = upsert(&json, &vec![entry("ubuntu_24.04", &[])]).unwrap();
        let parsed: Repositories = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(merged.len(), 2);
        let ids: Vec<&str> = parsed.iter().map(|e| e["distro_id"].as_str()).collect();
        assert!(ids.contains(&"fedora_42") && ids.contains(&"ubuntu_24.04"));
    }

    #[test]
    fn test_upsert_updates_existing_distro() {
        let (json, _) = upsert("", &vec![entry("arch", &[("download_url", "old")])]).unwrap();
        let (json, _) = upsert(&json, &vec![entry("arch", &[("download_url", "new")])]).unwrap();
        let parsed: Repositories = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1, "re-upserting same distro must not duplicate");
        assert_eq!(parsed[0]["download_url"], "new");
    }
}
