use crate::publish::store::Store;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

#[derive(Debug, PartialEq)]
pub struct Pruned {
    pub kept: usize,
    pub removed: usize,
}

// each dir holds one subdirectory per version: <dir>/<version>/<file>
pub fn prune(store: &Store, dirs: &[&str], current: &str, keep_previous: usize) -> Result<Pruned> {
    let mut files = vec![];
    let mut newest: HashMap<String, SystemTime> = HashMap::new();
    for dir in dirs {
        let scoped = store.scoped(dir);
        for (key, modified) in scoped.list_modified()? {
            let Some((version, _)) = key.split_once('/') else {
                continue;
            };
            let t = newest.entry(version.to_string()).or_insert(modified);
            *t = (*t).max(modified);
            files.push((scoped.clone(), version.to_string(), key));
        }
    }

    let stale = stale_versions(&newest, current, keep_previous);
    for (scoped, version, key) in &files {
        if stale.contains(version) {
            scoped.delete(key)?;
        }
    }
    Ok(Pruned {
        kept: newest.len() - stale.len(),
        removed: stale.len(),
    })
}

fn stale_versions(newest: &HashMap<String, SystemTime>, current: &str, keep_previous: usize) -> HashSet<String> {
    let mut previous: Vec<(&String, &SystemTime)> = newest.iter().filter(|(v, _)| v.as_str() != current).collect();
    previous.sort_by(|a, b| b.1.cmp(a.1).then_with(|| b.0.cmp(a.0)));
    previous.into_iter().skip(keep_previous).map(|(v, _)| v.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn keeps_current_and_newest_previous_versions() {
        let newest = HashMap::from([("1.0".to_string(), at(1)), ("1.1".to_string(), at(2)), ("1.2".to_string(), at(3)), ("0.9".to_string(), at(10))]);
        assert_eq!(stale_versions(&newest, "1.2", 0), HashSet::from(["1.0".to_string(), "1.1".to_string(), "0.9".to_string()]));
        assert_eq!(stale_versions(&newest, "1.2", 1), HashSet::from(["1.0".to_string(), "1.1".to_string()]));
        assert_eq!(stale_versions(&newest, "1.0", 5), HashSet::new());
        assert_eq!(stale_versions(&newest, "2.0", 0).len(), 4, "current not published yet: everything else is stale");
    }

    #[test]
    fn prune_deletes_stale_versions_across_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::LocalFs { root: dir.path().to_path_buf() };
        for (key, body) in [
            ("bottles/1.0/a-1.0.arm64_sequoia.bottle.tar.gz", "a"),
            ("meta/1.0/arm64_sequoia.json", "{}"),
            ("bottles/1.1/a-1.1.arm64_sequoia.bottle.tar.gz", "a"),
            ("bottles/1.1/a-1.1.arm64_tahoe.bottle.tar.gz", "a"),
            ("meta/1.1/arm64_sequoia.json", "{}"),
            ("bottles/flat-legacy.tar.gz", "a"),
        ] {
            store.put(key, body.as_bytes().to_vec(), None).unwrap();
        }

        assert_eq!(prune(&store, &["bottles", "meta"], "1.1", 0).unwrap(), Pruned { kept: 1, removed: 1 });
        assert!(!dir.path().join("bottles/1.0").exists());
        assert!(!dir.path().join("meta/1.0").exists());
        assert!(dir.path().join("bottles/1.1/a-1.1.arm64_tahoe.bottle.tar.gz").exists());
        assert!(dir.path().join("meta/1.1/arm64_sequoia.json").exists());
        assert!(dir.path().join("bottles/flat-legacy.tar.gz").exists());
        assert!(dir.path().join("bottles").exists());
    }
}
