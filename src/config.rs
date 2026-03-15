use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ExtractVersionFile {
    pub file: String,
    pub regex: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ExtractVersion {
    pub provider: String,
    pub file: Option<ExtractVersionFile>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct RpmConfig {
    pub spec_template: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DebConfig {
    pub debian_templates: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Build {
    pub distro: String,
    pub package_name: String,
    pub maintainer: String,
    pub homepage: String,
    pub description: String,
    #[serde(default)]
    pub build_dependencies: Vec<String>,
    #[serde(default)]
    pub runtime_dependencies: Vec<String>,
    pub before_build_script: Option<String>,
    pub rpm: Option<RpmConfig>,
    pub deb: Option<DebConfig>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Config {
    pub extract_version: ExtractVersion,
    pub builds: Vec<Build>,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config = serde_saphyr::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let path = std::path::Path::new("tests/fixtures/config.yml");
        let config = Config::load(path).unwrap();

        assert_eq!(config.extract_version.provider, "file");
        let file = config.extract_version.file.as_ref().unwrap();
        assert_eq!(file.file, "lib/omnipackage_agent/version.rb");
        assert_eq!(file.regex, "VERSION = '(.+)'");

        assert!(!config.builds.is_empty());

        let first = &config.builds[0];
        assert_eq!(first.distro, "opensuse_15.3");
        assert_eq!(first.package_name, "omnipackage-agent");
        assert!(!first.build_dependencies.is_empty());
        assert!(first.rpm.is_some());
        assert!(first.deb.is_none());

        let deb_build = config.builds.iter().find(|b| b.distro == "debian_10").unwrap();
        assert!(deb_build.deb.is_some());
        assert!(deb_build.rpm.is_none());

        // verify merge key resolution — fields from anchors are present
        let simple_rpm = config.builds.iter().find(|b| b.distro == "fedora_38").unwrap();
        assert_eq!(simple_rpm.package_name, "omnipackage-agent");
        assert_eq!(simple_rpm.homepage, "https://omnipackage.org/");
    }
}
