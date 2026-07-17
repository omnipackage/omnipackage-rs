use super::{Repositories, sorted_by_distro_order, upsert_repository};
use serde_json::{Map, Value};

pub fn upsert(existing_json: &str, entries: &Repositories) -> Result<(String, Repositories), anyhow::Error> {
    let mut repos = from_json(existing_json);

    for entry in entries {
        upsert_repository(&mut repos, entry.clone());
    }

    let merged = sorted_by_distro_order(&repos);
    let json = to_json(&merged)?;
    Ok((json, merged))
}

fn from_json(existing_json: &str) -> Repositories {
    let trimmed = existing_json.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let objects: Vec<Map<String, Value>> = serde_json::from_str(trimmed).unwrap_or_default();
    objects
        .into_iter()
        .map(|obj| {
            obj.into_iter()
                .map(|(k, v)| {
                    let s = match v {
                        Value::Array(items) => items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    (k, s)
                })
                .collect()
        })
        .collect()
}

fn to_json(repos: &Repositories) -> Result<String, anyhow::Error> {
    let objects: Vec<Map<String, Value>> = repos
        .iter()
        .map(|entry| {
            entry
                .iter()
                .map(|(k, v)| {
                    let value = if k == "install_steps" {
                        Value::Array(v.split('\n').map(|line| Value::String(line.to_string())).collect())
                    } else {
                        Value::String(v.clone())
                    };
                    (k.clone(), value)
                })
                .collect()
        })
        .collect();
    Ok(serde_json::to_string_pretty(&objects)?)
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
        let parsed: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["arch"], "x86_64");
        assert_eq!(parsed[0]["package_name"], "omnipackage");
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn test_upsert_merges_two_distros() {
        let (json, _) = upsert("", &vec![entry("fedora_42", &[])]).unwrap();
        let (json, merged) = upsert(&json, &vec![entry("ubuntu_24.04", &[])]).unwrap();
        let parsed: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(merged.len(), 2);
        let ids: Vec<&str> = parsed.iter().map(|e| e["distro_id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"fedora_42") && ids.contains(&"ubuntu_24.04"));
    }

    #[test]
    fn test_upsert_updates_existing_distro() {
        let (json, _) = upsert("", &vec![entry("arch", &[("download_url", "old")])]).unwrap();
        let (json, _) = upsert(&json, &vec![entry("arch", &[("download_url", "new")])]).unwrap();
        let parsed: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1, "re-upserting same distro must not duplicate");
        assert_eq!(parsed[0]["download_url"], "new");
    }

    #[test]
    fn test_install_steps_serialized_as_array() {
        let (json, merged) = upsert("", &vec![entry("ubuntu_24.04", &[("install_steps", "line one\nline two\nline three")])]).unwrap();
        let parsed: Vec<Value> = serde_json::from_str(&json).unwrap();
        let steps = &parsed[0]["install_steps"];
        assert!(steps.is_array(), "install_steps not an array: {steps}");
        assert_eq!(steps.as_array().unwrap().len(), 3);
        assert_eq!(steps[0], "line one");
        assert_eq!(steps[2], "line three");
        assert_eq!(merged[0]["install_steps"], "line one\nline two\nline three", "merged in-memory form stays a joined string");
    }

    #[test]
    fn test_reads_back_array_steps() {
        let (json, _) = upsert("", &vec![entry("arch", &[("install_steps", "a\nb")])]).unwrap();
        let (json, _) = upsert(&json, &vec![entry("fedora_42", &[("install_steps", "c")])]).unwrap();
        let parsed: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        let arch = parsed.iter().find(|e| e["distro_id"] == "arch").unwrap();
        assert_eq!(arch["install_steps"].as_array().unwrap().len(), 2, "existing array steps not preserved across re-upsert");
    }
}
