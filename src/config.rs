use crate::build::package::template::Var;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct ExtractVersionFile {
    pub file: String,
    pub regex: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ExtractVersion {
    pub provider: String,
    pub file: Option<ExtractVersionFile>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RpmConfig {
    pub spec_template: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DebConfig {
    pub debian_templates: String,
}

#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub extract_version: ExtractVersion,
    pub builds: Vec<Build>,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let content = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
        serde_saphyr::from_str(&content).unwrap_or_else(|e| panic!("cannot parse {}: {}", path.display(), e))
    }
}

impl Build {
    pub fn to_vars(&self) -> HashMap<String, Var> {
        let mut vars = HashMap::new();
        vars.insert("package_name".to_string(), self.package_name.clone().into());
        vars.insert("maintainer".to_string(), self.maintainer.clone().into());
        vars.insert("homepage".to_string(), self.homepage.clone().into());
        vars.insert("description".to_string(), self.description.clone().into());
        vars.insert("build_dependencies".to_string(), self.build_dependencies.clone().into());
        vars.insert("runtime_dependencies".to_string(), self.runtime_dependencies.clone().into());
        vars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let path = Path::new("tests/fixtures/config.yml");
        let config = Config::load(path);

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
