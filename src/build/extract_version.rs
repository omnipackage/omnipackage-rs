use crate::config::ExtractVersion;
use regex::Regex;
use std::error::Error;
use std::path::Path;

pub fn extract_version(path: &Path, config: &ExtractVersion) -> Result<String, Box<dyn Error>> {
    match config.provider.as_str() {
        "file" => {
            let file_config = config.file.clone().ok_or("file config is missing")?;

            let file_path = path.join(&file_config.file);
            let content = std::fs::read_to_string(&file_path).map_err(|e| format!("cannot read {}: {}", file_path.display(), e))?;

            let regex = &file_config.regex;
            let re = Regex::new(regex).map_err(|e| format!("invalid regex '{}': {}", regex, e))?;

            let result = re
                .captures(&content)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .ok_or_else(|| format!("regex '{}' did not match in {}", regex, file_path.display()));
            Ok(result?)
        }
        _ => Err(format!("unknown version provider {}", config.provider).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ExtractVersion, ExtractVersionFile};
    use std::path::PathBuf;

    fn make_config(file: &str, regex: &str) -> ExtractVersion {
        ExtractVersion {
            provider: "file".to_string(),
            file: Some(ExtractVersionFile {
                file: file.to_string(),
                regex: regex.to_string(),
            }),
            shell: None,
        }
    }

    #[test]
    fn test_extract_version_from_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("version.rb"), "VERSION = '1.2.3'").unwrap();

        let config = make_config("version.rb", "VERSION = '(.+)'");
        let version = extract_version(&dir.path().to_path_buf(), &config).unwrap();
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn test_extract_version_unknown_provider() {
        let config = ExtractVersion {
            provider: "unknown".to_string(),
            file: None,
            shell: None,
        };
        let result = extract_version(&PathBuf::from("."), &config);
        assert!(result.is_err());
    }
}
