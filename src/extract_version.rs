use crate::config::VersionExtractor;
use crate::shell::Command;
use regex::Regex;
use std::error::Error;
use std::path::Path;

pub fn extract_version(path: &Path, config: &VersionExtractor) -> Result<String, Box<dyn Error>> {
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
        "shell" => {
            let shell_config = config.shell.clone().ok_or("shell config is missing")?;
            let output = Command::new("sh").args(["-c", &shell_config.command]).capture()?.trim_end().to_string();
            Ok(output)
        }
        "constant" => {
            let constant_config = config.constant.clone().ok_or("constant config is missing")?;
            Ok(constant_config.version)
        }
        _ => Err(format!("unknown version provider '{}'", config.provider).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ExtractVersionConstant, ExtractVersionFile, VersionExtractor};
    use std::path::PathBuf;

    fn make_config(file: &str, regex: &str) -> VersionExtractor {
        VersionExtractor {
            provider: "file".to_string(),
            name: "testtest".to_string(),
            file: Some(ExtractVersionFile {
                file: file.to_string(),
                regex: regex.to_string(),
            }),
            shell: None,
            constant: None,
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
    fn test_extract_version_constant() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("version.rb"), "VERSION = '1.2.3'").unwrap();

        let config = make_config("version.rb", "VERSION = '(.+)'");
        let version = extract_version(&dir.path().to_path_buf(), &config).unwrap();
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn test_extract_version_unknown_provider() {
        let config = VersionExtractor {
            provider: "constant".to_string(),
            name: "onono".to_string(),
            file: None,
            shell: None,
            constant: Some(ExtractVersionConstant {
                version: "some static string".to_string(),
            }),
        };
        let version = extract_version(&PathBuf::from("."), &config).unwrap();
        assert_eq!(version, "some static string");
    }
}
