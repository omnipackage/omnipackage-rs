use crate::build::package::template::Var;
use crate::logger::Logger;
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
#[serde(untagged)]
pub enum AnyValue {
    String(String),
    Bool(bool),
    Int(i64),
    Float(f64),
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
    #[serde(flatten, default)]
    pub rest: HashMap<String, AnyValue>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub extract_version: ExtractVersion,
    pub builds: Vec<Build>,
}

impl Config {
    #[allow(dead_code)]
    pub fn load(path: &Path) -> Self {
        Self::load_with_env(path, Path::new(".env"))
    }

    pub fn load_with_env(path: &Path, env_path: &Path) -> Self {
        let env_map: std::collections::HashMap<String, String> = match dotenvy::from_path_iter(env_path) {
            Ok(iter) => {
                let map: std::collections::HashMap<String, String> = iter.filter_map(|e| e.ok()).collect();
                Logger::new().info(format!(
                    "env loaded from {}: {}",
                    std::env::current_dir().unwrap_or_default().join(env_path).display(),
                    map.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
                map
            }
            Err(_) => std::collections::HashMap::new(),
        };

        let content = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));

        let content = Self::expand_env_vars_with(&content, |var| env_map.get(var).cloned().or_else(|| std::env::var(var).ok()).unwrap_or_default());

        serde_saphyr::from_str(&content).unwrap_or_else(|e| panic!("cannot parse {}: {}", path.display(), e))
    }

    fn expand_env_vars_with<F>(content: &str, resolver: F) -> String
    where
        F: Fn(&str) -> String,
    {
        let re = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();
        re.replace_all(content, |caps: &regex::Captures| resolver(&caps[1])).to_string()
    }
}

impl Build {
    pub fn to_template_vars(&self) -> HashMap<String, Var> {
        let mut vars = HashMap::new();
        vars.insert("package_name".to_string(), self.package_name.clone().into());
        vars.insert("maintainer".to_string(), self.maintainer.clone().into());
        vars.insert("homepage".to_string(), self.homepage.clone().into());
        vars.insert("description".to_string(), self.description.clone().into());
        vars.insert("build_dependencies".to_string(), self.build_dependencies.clone().into());
        vars.insert("runtime_dependencies".to_string(), self.runtime_dependencies.clone().into());

        for (k, v) in &self.rest {
            let var = match v {
                AnyValue::String(s) => s.clone().into(),
                AnyValue::Bool(b) => (*b).into(),
                AnyValue::Int(i) => (*i).into(),
                AnyValue::Float(f) => f.to_string().into(),
            };
            vars.insert(k.clone(), var);
        }

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

    #[test]
    fn test_extra_fields_in_template_vars() {
        let yaml = r#"
    distro: test
    package_name: myapp
    maintainer: Test <test@test.com>
    homepage: https://example.com
    description: Test
    custom_string: hello
    custom_bool: true
    "#;

        let build: Build = serde_saphyr::from_str(yaml).unwrap();
        let vars = build.to_template_vars();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.liquid");
        std::fs::write(&path, "{{ description }} {{ custom_string }} {{ custom_bool }}").unwrap();

        let template = crate::build::package::template::Template::new(path);
        let output = template.render(vars);
        assert_eq!(output, "Test hello true");
    }

    #[test]
    fn test_expand_env_vars_basic() {
        let result = Config::expand_env_vars_with("value: ${FOO} and ${BAR}", |var| match var {
            "FOO" => "hello".to_string(),
            "BAR" => "world".to_string(),
            _ => String::new(),
        });
        assert_eq!(result, "value: hello and world");
    }

    #[test]
    fn test_expand_env_vars_missing() {
        let result = Config::expand_env_vars_with("value: ${MISSING}", |_| String::new());
        assert_eq!(result, "value: ");
    }

    #[test]
    fn test_expand_env_vars_no_placeholders() {
        let result = Config::expand_env_vars_with("plain: value", |_| String::new());
        assert_eq!(result, "plain: value");
    }

    #[test]
    fn test_expand_env_vars_multiple_same() {
        let result = Config::expand_env_vars_with("${FOO} and ${FOO}", |_| "bar".to_string());
        assert_eq!(result, "bar and bar");
    }

    #[test]
    fn test_load_expands_env_vars() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yml");
        let env_path = dir.path().join(".env");

        std::fs::write(&env_path, "MY_VAR=expanded_value").unwrap();
        std::fs::write(
            &config_path,
            "
    extract_version:
      provider: file
      file:
        file: ${MY_VAR}
        regex: VERSION
    builds: []
    ",
        )
        .unwrap();

        let config = Config::load_with_env(&config_path, &env_path);
        assert_eq!(config.extract_version.file.unwrap().file, "expanded_value");
    }
}
