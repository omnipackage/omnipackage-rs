use crate::config::Config;
use regex::Regex;
use std::path::PathBuf;

pub fn extract_version(path: &PathBuf, config: &Config) -> String {
    match config.extract_version.provider.as_str() {
        "file" => {
            let file_config = &config.extract_version.file.clone().unwrap_or_else(|| panic!("cannot read file config"));

            let file_path = path.join(&file_config.file);
            let content = std::fs::read_to_string(&file_path).unwrap_or_else(|e| panic!("cannot read {}: {}", file_path.display(), e));

            let regex = &file_config.regex;
            let re = Regex::new(regex).unwrap_or_else(|e| panic!("invalid regex '{}': {}", regex, e));

            re.captures(&content)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| panic!("regex '{}' did not match in {}", regex, file_path.display()))
        }
        _ => panic!("unknown version provider {}", config.extract_version.provider),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ExtractVersion, ExtractVersionFile};

    fn make_config(file: &str, regex: &str) -> Config {
        Config {
            extract_version: ExtractVersion {
                provider: "file".to_string(),
                file: Some(ExtractVersionFile {
                    file: file.to_string(),
                    regex: regex.to_string(),
                }),
            },
            builds: vec![],
        }
    }

    #[test]
    fn test_extract_version_from_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("version.rb"), "VERSION = '1.2.3'").unwrap();

        let config = make_config("version.rb", "VERSION = '(.+)'");
        let version = extract_version(&dir.path().to_path_buf(), &config);
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn test_extract_version_unknown_provider() {
        let config = Config {
            extract_version: ExtractVersion {
                provider: "unknown".to_string(),
                file: None,
            },
            builds: vec![],
        };
        let result = std::panic::catch_unwind(|| extract_version(&std::path::PathBuf::from("."), &config));
        assert!(result.is_err());
    }
}
