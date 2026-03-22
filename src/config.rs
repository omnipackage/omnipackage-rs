use crate::build::package::template::Var;
use crate::logger::Logger;
use base64::{Engine, engine::general_purpose};
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
pub struct LocalFsConfig {
    pub path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub path_in_bucket: Option<String>,
    pub bucket_public_url: Option<String>,
    pub endpoint: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: Option<String>,
    #[serde(default)]
    pub force_path_style: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Repository {
    pub name: String,
    pub provider: String,
    pub localfs: Option<LocalFsConfig>,
    pub s3: Option<S3Config>,
    pub gpg_private_key_base64: String,
    pub package_name: String,
}

impl Repository {
    pub fn localfs(&self) -> &LocalFsConfig {
        self.localfs.as_ref().unwrap_or_else(|| panic!("repository '{}' has no localfs config", self.name))
    }

    pub fn s3(&self) -> &S3Config {
        self.s3.as_ref().unwrap_or_else(|| panic!("repository '{}' has no s3 config", self.name))
    }

    pub fn gpg_private_key(&self) -> Result<String, String> {
        let decoded = general_purpose::STANDARD
            .decode(self.gpg_private_key_base64.clone())
            .map_err(|e| format!("cannot decode GPG key: {}", e))?;
        String::from_utf8(decoded).map_err(|e| format!("invalid UTF-8 in GPG key: {}", e))
    }
}

impl S3Config {
    pub fn base_url(&self) -> &str {
        let url = self.bucket_public_url.as_deref().unwrap_or(&self.endpoint);
        // TODO: handle different providers' shenanigans and/or force_path_style
        url.trim_end_matches('/')
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Repositories(Vec<Repository>);

impl Repositories {
    pub fn find_by_name_or_default(&self, name: Option<&str>) -> Result<&Repository, String> {
        match name {
            Some(name) => self.0.iter().find(|r| r.name == name).ok_or_else(|| format!("repository '{}' not found in config", name)),
            None => self.0.first().ok_or_else(|| "no repositories configured".to_string()),
        }
    }
}

impl std::ops::Deref for Repositories {
    type Target = Vec<Repository>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub extract_version: ExtractVersion,
    pub builds: Vec<Build>,
    #[serde(default)]
    pub repositories: Repositories,
    #[serde(default)]
    pub secrets: HashMap<String, String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        Self::load_with_env(path, Path::new(".env"))
    }

    pub fn load_with_env(path: &Path, env_path: &Path) -> Result<Self, String> {
        let env_map: HashMap<String, String> = match dotenvy::from_path_iter(env_path) {
            Ok(iter) => {
                let map: HashMap<String, String> = iter.filter_map(|e| e.ok()).collect();
                Logger::new().info(format!(
                    "env loaded from {}: {}",
                    std::env::current_dir().unwrap_or_default().join(env_path).display(),
                    map.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
                map
            }
            Err(_) => {
                Logger::new().warn(format!("no env in {}", std::env::current_dir().unwrap_or_default().join(env_path).display()));
                HashMap::new()
            }
        };

        let content = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;

        let content = Self::expand_env_vars_with(&content, |var| env_map.get(var).cloned().or_else(|| std::env::var(var).ok()).unwrap_or_default());

        serde_saphyr::from_str(&content).map_err(|e| format!("cannot parse {}: {}", path.display(), e))
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

    pub fn build_folder_name(&self) -> String {
        format!("{}-{}", self.package_name, self.distro)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let path = Path::new("tests/fixtures/config.yml");
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

        let config = Config::load_with_env(&config_path, &env_path).unwrap();
        assert_eq!(config.extract_version.file.unwrap().file, "expanded_value");
    }

    #[test]
    fn test_load_repositories() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yml");

        std::fs::write(
            &config_path,
            r#"
    extract_version:
      provider: file
      file:
        file: version.rb
        regex: VERSION
    builds: []
    repositories:
      - name: Local test
        package_name: ololo
        gpg_private_key_base64: "LS0tLS1CRUdJTiBQR1AgUFJJVkFURSBLRVkgQkxPQ0stLS0tLQoKbFFjWUJHbStVcTRCRUFEUm5uU29uRlVLaUNEdFlSTmZuRytOTGYvQnVXSktyR0IxTGxSZTJNUEE3dFcvc1RuUgptcEdnK3B1dTJOL29VY2ZlYklwZExzYkRXSmIzS0pPMkJhU04vQmZVcGFhWkdvZjN0MzlrL2Rab2dKaDJQRjJlCjJPVWk1M1E0aVlUa2t5ZXMrb01BYmdwSmV5a1lBSlZvMTl5UC8va0UyZEhsNEFmaFhwMHBGeU1salJWeVI4eUoKdS9KbHpqSzVYLzQ1OWo3Wjk2cXpJRVdoazRIa0ZKSG9LYU8xb0FXR3l4RkNJMlhiYlcyemVqYlRwOXpFK2hSUAo2VmIxbmJrd3BXdE14Z0VPM0RPMFVMT1RTTmkycFdnaEJFUVN2M2plUVV0aGpyMUwvdnd4SXFlUFU0YlVkdGhtClVuN0c4Tnl1V2pVTUpyemwyVnZZR0lUTHd6UG1HM3libVZHQ0JoWjc4NENiSC9HTzRhWkYwR2xIZy96dTQ2NUEKa21XZTRRWE5RcHVLSm9JZlFRakFEZGFDOW03bitBK0c4dU1OTkpIUEZTY200V2hZTTFaWHhGUFRlUE9Ldzh0NwptcThoY0RNNnVIU0IvcTdkZkV2OVRHQW9zcWIzVGJ5SktxNmVwLzhkTkc5b1E3d2VMcVBiTXBDZVQ4SUY0bEhWCmRsQmsrVEZScHZrSGZHZUZWSTRFejBEaTM5VzJEZEpja01LZlRva1BzRFpPWGxYbThFZUtIeTR6R05NNnRxcGsKVXB0ZkZwZU56TEJGMXJWSjFXV0dMZTRrR0gycGt3L1Iwc29TVmxQM1JraURHK0NKWnk3Z0ZEU09hTGphUTVaZgpLOEtzMGhqSmN1S1VMRmhtS2plcVpzbmVxaHg3MnpwNEgxY3o5UzJqSzYxVis3TUFWTGFOSUF2SVR3QVJBUUFCCkFBLzlHcTlZWElxaVROZnRrU3FWZzh0dmJBS2FGM2haS2ZadHhSMGp3bnRIMkV6UFN6cnRpR0JyLzVsSHJtZVAKZ1l5L3EvSVhYeXR3UkRnSDUranpmTzJVK0RwS2NsaXdMN2R2N2JvZmJxVGQ0YW5WUHVBS2J3bGZpaVQ2NTZPaQpGbE1oYnVUcFBtbkR4M3oxUzBmdnZVd1ZtUU5XR1NiOWtJMEhrOG91cWFkM1l2Vmw4SWx4WVd0YXZWa1ZuZ0c1ClpIbVRvTDNscmk0Q2owSXUrL0VlYmZhM0Q0MFNod2xjNWhXSmh2aVVTWS9hWVBGeXFtVEhwekNtVUZoRzFnbUkKOGxvV2ttUzM5RkY5dXRkQU4yZ3hMRnJWcTZsU1lzRGZMc2swRWZPZytVUlhIbWpjdGpsMjdQZTlpZWZwd0F4ZAp5bkU0Qk82NXdYWEZtZ2I4bVdPUVptS0xQSTM5QmE1em53Z3U2U1hIOEtoRUZld1oyOUh0MjNPM0piY21iVDl5CmRMZm9Ya0Y5elNRTTlicVRaUEdTMDZwaFkwT0tNalhRWkNGU29aR2dYTUFqWGd3dEtyeU9PbVNraktBcHphaTMKQnpacU51d3FxM1FHaDdzUVRpQ1JtelVyMS93SHJUTXJzejJnTUhXbllPaVlUSkNOSkt2b1R4MlV1U3ZzZU1BRwpXSE9KRUI0WE5TRHlMblE1UDVvTW5CWGtsV2xwdWJFaGIzNDFRbFRxZWVzamJiQTFia3F5ZzBLZkM2VzRaQWhtCnFraXFRTE9KbVN0UVZ3enQ2Ykt5MnhxOXZ1cjRSWXJiVjlsRG9XTzZtTTRpTFVQNmFERGhpc00xRUVvSnJ6M2oKdG5HUlBpY2h3cXpJYWViQllKTUxQdnd5N1BGRUY4VmZXQ2JaU2Z4NDhsN2crK0VJQU5OT0xHdDd1elJaODd3Rgp3VU9laVdDZ0FHZXpjM1dNMDh6RTh3aXlGMmdEN2I5TElsRzQwL0NQdjZOZlRLbjF2Z1lpazhtZlBuc1l6aU4vCnBIcmF2eFRvd3hGeXNaMDBOcWVKUEFTdVVmK3dIQmN5TUdDQmZqV2UycnE3WEduc2p0Uzc5YWtiS29RNm9YRSsKeWs5N0RSYWRvWFVvRFlDV0pWZU55Sk9hL1Q4NVBLeE1RN3JJclVoZThCUGlhbDhtZjIvYm1oRk1ISGFBUHUzSQpkRnN5SWcyQnRMVDZPZEVveEVYY3pYbGNhT2FRdVRBRnU3dVE5dzhsWUhkQ3RBN3laSU01aGFURGVJVllCRjROCmRNaDl3bVplbWZBbUVNTjNHd2trV1B3aU5CZ0F4V2tPY3dadmpiNno1MFhud2puQ3dkeGpoTFpyUlNsbGhqTTEKSUhoNkRyOElBUDMwOTNQWm9DSW1uYXdzNm9uaWhISWJIYTcwMWpWWVpZRXR1VTVEeHkwclA1aGlUNFpha1h6MApRS0pEL3hhYVdYNWFiMjF6QzB2dDczOGlGSkpuKzVUREtsTXFOaS8xTDYzZEIwOEVKRHI0VlFHQy80OWJXQno5CkIxZ3cxYVdSOGFLNjRNS1FacFE2UkNIRXVRSDZJV253ZDZuNjlMdFhEYkFoL3ZNdExRSVJIUlNucG9DblBIdTAKSmNPTzIwSFNSZXNwYzdFckN1cXFTRnh3TlVwUGNRRm5XdnV1Rks5WG1COVBvN3ZveUx6U2NUdEorN0twc1JBdwpTTmduNHdSSXlzeW5YSlBXaEhxQTg0VHBiek54ZC9XNGVsZGYzdERmZTlldkpUVkZ1REpNeGpWdFI3N0VHRTA0CnkyOGdZeUNXdGtXT2x4QnM4a2phaWRGa0FMNHNPbkVIL2pnOGN1M21rQk42QnpXRWpaMytXZnNvWnRqT0Q5VFMKWXVVVXh2cFBzRFdReXd2TmlKS1ZWcWJXKzhsbWpKNW11L2FjUEI1MXc1aVJpRTlscnB6VGV2dlZPd3JVUmN0Mgo2d2d1cnJyUkF0cndHZGVVZVNWWUxUVTNuS1pxUG8reElETm1IeTIyL3V1ZWtLSElKdUUyWDRuT1JDUzRyQ2lVClU0VXp2OWQxQklYV0dKVkRRdHBlOTlrQ2pRQmowWlhOR0dTNzRPd3JGWnU1ZElIcVdORnM5TFdIRThOWE4xankKTDRUaCtTcU5wbXhSMW5GdG5FM2dZbTNMdTVUVGl2NVh1WWpZLzVSaFNKMkxDRUpOSFd1cm5KZW0vZjdqcTBGagpaNEZsVEgzLzE0SjJLZFZCeFVhNXIrcWwrajhVYm5OSVlQNUZ6WHd6OHdlMWdiWllNVTM1azM1OStyUWdUMnh2CmJHVm5JRHh2Ykc5c2IyVm5RRzl0Ym1sd1lXTnJZV2RsTG05eVp6NkpBbTRFRXdFSUFGZ1dJUVNrb1Fjc0w1RDcKSDl1di9EeTZrZ0o0R3h2WUlBVUNhYjVTcmhzVWdBQUFBQUFFQUE1dFlXNTFNaXd5TGpVck1TNHhNaXd5TERJRApHeThFQlFzSkNBY0NBaUlDQmhVS0NRZ0xBZ1FXQWdNQkFoNEhBaGVBQUFvSkVMcVNBbmdiRzlnZ3Nlc1AvMHg0CjRoUDcraFVSUERXeTJPdWhCQk9VcU8rTGpwWGJRM24rUlRpb05Yekt2Q1hYdk1ucDhXczR1TDZJdEdwNmVydXUKdUlQdm81UWQ5ak9KMUh0UkJvVDg2b2tQeDVsNkU1OXVSaTBlRVdocXo2WkExSm5IZURHMFBjTmd1SE1aYVdOVAo4anRacjBucTRZaGlvb0RRN0NSUG1zMEptdGw4cHM1M3QxY0dSemRrM3RPbWxkcUtpbWp5YUp1L1NweExGMkMrCk9wTHdWREg1L3R3K3grZWthdG16SldqdmZVRVQyUzQzcHZaendGSUVaTEdNMEl6VkFORWRJUUxUMTE3OXkzM1kKSXRGRG0wQmpVN0F4bVR5NE9WZzFER0l5Qjk1RU51UmhZWGwvYXJ1N1BYVCtuWnVCTms3SXdJalpqckdFTXJpbgpRdXdaWDNVaStuK0hmQTJFMGxKdEZPQUpJRFZZM1lIY0xHamtYK3ZFYUt5RFhvUlJiVVZ4QzVuMU9QZDdCelN2CmphcE9EMHRxNk5BTW04QjAwZVVuTVNxUEN1dFdIUzRpbU1rZ2ZEK2xwajVaV3FIY3ZwVEU0UGFiTWU5OHA2akQKS3dpajRmRTBxYlhGYThkMnl4cFJTdk1yZGpQMHQydDNNQzgwNzltSWtqNWJjeEhIMUhtV1NzL0VtZmdFbWJEUQpBTllQSG1Vbmo3ZUZSYUlhc2h5NXBkTStMTVk3emZEWUgyUDkydUZBajE3a1lpdHY1LzN1bXBhSlRvV25leFp0Ckd5T3dFR2hlVU1uVklxTlJKM3BWbnNvVXkzdy9hOXRwY0JHSm04NHdVakhoL1pWaVY5RmVVOUZjR3F3c1JwRWUKZ1JZa2RzSFJOVzJ5ZnVQdU42UExaZkpvSW9wNWF5ZllqbU1Ob3dNZQo9N01iZQotLS0tLUVORCBQR1AgUFJJVkFURSBLRVkgQkxPQ0stLS0tLQo="
        provider: localfs
        localfs:
          path: /tmp/omnipackage-repos
      - name: CloudFlare R2
        package_name: ololo
        gpg_private_key_base64: "LS0tLS1CRUdJTiBQR1AgUFJJVkFURSBLRVkgQkxPQ0stLS0tLQoKbFFjWUJHbStVcTRCRUFEUm5uU29uRlVLaUNEdFlSTmZuRytOTGYvQnVXSktyR0IxTGxSZTJNUEE3dFcvc1RuUgptcEdnK3B1dTJOL29VY2ZlYklwZExzYkRXSmIzS0pPMkJhU04vQmZVcGFhWkdvZjN0MzlrL2Rab2dKaDJQRjJlCjJPVWk1M1E0aVlUa2t5ZXMrb01BYmdwSmV5a1lBSlZvMTl5UC8va0UyZEhsNEFmaFhwMHBGeU1salJWeVI4eUoKdS9KbHpqSzVYLzQ1OWo3Wjk2cXpJRVdoazRIa0ZKSG9LYU8xb0FXR3l4RkNJMlhiYlcyemVqYlRwOXpFK2hSUAo2VmIxbmJrd3BXdE14Z0VPM0RPMFVMT1RTTmkycFdnaEJFUVN2M2plUVV0aGpyMUwvdnd4SXFlUFU0YlVkdGhtClVuN0c4Tnl1V2pVTUpyemwyVnZZR0lUTHd6UG1HM3libVZHQ0JoWjc4NENiSC9HTzRhWkYwR2xIZy96dTQ2NUEKa21XZTRRWE5RcHVLSm9JZlFRakFEZGFDOW03bitBK0c4dU1OTkpIUEZTY200V2hZTTFaWHhGUFRlUE9Ldzh0NwptcThoY0RNNnVIU0IvcTdkZkV2OVRHQW9zcWIzVGJ5SktxNmVwLzhkTkc5b1E3d2VMcVBiTXBDZVQ4SUY0bEhWCmRsQmsrVEZScHZrSGZHZUZWSTRFejBEaTM5VzJEZEpja01LZlRva1BzRFpPWGxYbThFZUtIeTR6R05NNnRxcGsKVXB0ZkZwZU56TEJGMXJWSjFXV0dMZTRrR0gycGt3L1Iwc29TVmxQM1JraURHK0NKWnk3Z0ZEU09hTGphUTVaZgpLOEtzMGhqSmN1S1VMRmhtS2plcVpzbmVxaHg3MnpwNEgxY3o5UzJqSzYxVis3TUFWTGFOSUF2SVR3QVJBUUFCCkFBLzlHcTlZWElxaVROZnRrU3FWZzh0dmJBS2FGM2haS2ZadHhSMGp3bnRIMkV6UFN6cnRpR0JyLzVsSHJtZVAKZ1l5L3EvSVhYeXR3UkRnSDUranpmTzJVK0RwS2NsaXdMN2R2N2JvZmJxVGQ0YW5WUHVBS2J3bGZpaVQ2NTZPaQpGbE1oYnVUcFBtbkR4M3oxUzBmdnZVd1ZtUU5XR1NiOWtJMEhrOG91cWFkM1l2Vmw4SWx4WVd0YXZWa1ZuZ0c1ClpIbVRvTDNscmk0Q2owSXUrL0VlYmZhM0Q0MFNod2xjNWhXSmh2aVVTWS9hWVBGeXFtVEhwekNtVUZoRzFnbUkKOGxvV2ttUzM5RkY5dXRkQU4yZ3hMRnJWcTZsU1lzRGZMc2swRWZPZytVUlhIbWpjdGpsMjdQZTlpZWZwd0F4ZAp5bkU0Qk82NXdYWEZtZ2I4bVdPUVptS0xQSTM5QmE1em53Z3U2U1hIOEtoRUZld1oyOUh0MjNPM0piY21iVDl5CmRMZm9Ya0Y5elNRTTlicVRaUEdTMDZwaFkwT0tNalhRWkNGU29aR2dYTUFqWGd3dEtyeU9PbVNraktBcHphaTMKQnpacU51d3FxM1FHaDdzUVRpQ1JtelVyMS93SHJUTXJzejJnTUhXbllPaVlUSkNOSkt2b1R4MlV1U3ZzZU1BRwpXSE9KRUI0WE5TRHlMblE1UDVvTW5CWGtsV2xwdWJFaGIzNDFRbFRxZWVzamJiQTFia3F5ZzBLZkM2VzRaQWhtCnFraXFRTE9KbVN0UVZ3enQ2Ykt5MnhxOXZ1cjRSWXJiVjlsRG9XTzZtTTRpTFVQNmFERGhpc00xRUVvSnJ6M2oKdG5HUlBpY2h3cXpJYWViQllKTUxQdnd5N1BGRUY4VmZXQ2JaU2Z4NDhsN2crK0VJQU5OT0xHdDd1elJaODd3Rgp3VU9laVdDZ0FHZXpjM1dNMDh6RTh3aXlGMmdEN2I5TElsRzQwL0NQdjZOZlRLbjF2Z1lpazhtZlBuc1l6aU4vCnBIcmF2eFRvd3hGeXNaMDBOcWVKUEFTdVVmK3dIQmN5TUdDQmZqV2UycnE3WEduc2p0Uzc5YWtiS29RNm9YRSsKeWs5N0RSYWRvWFVvRFlDV0pWZU55Sk9hL1Q4NVBLeE1RN3JJclVoZThCUGlhbDhtZjIvYm1oRk1ISGFBUHUzSQpkRnN5SWcyQnRMVDZPZEVveEVYY3pYbGNhT2FRdVRBRnU3dVE5dzhsWUhkQ3RBN3laSU01aGFURGVJVllCRjROCmRNaDl3bVplbWZBbUVNTjNHd2trV1B3aU5CZ0F4V2tPY3dadmpiNno1MFhud2puQ3dkeGpoTFpyUlNsbGhqTTEKSUhoNkRyOElBUDMwOTNQWm9DSW1uYXdzNm9uaWhISWJIYTcwMWpWWVpZRXR1VTVEeHkwclA1aGlUNFpha1h6MApRS0pEL3hhYVdYNWFiMjF6QzB2dDczOGlGSkpuKzVUREtsTXFOaS8xTDYzZEIwOEVKRHI0VlFHQy80OWJXQno5CkIxZ3cxYVdSOGFLNjRNS1FacFE2UkNIRXVRSDZJV253ZDZuNjlMdFhEYkFoL3ZNdExRSVJIUlNucG9DblBIdTAKSmNPTzIwSFNSZXNwYzdFckN1cXFTRnh3TlVwUGNRRm5XdnV1Rks5WG1COVBvN3ZveUx6U2NUdEorN0twc1JBdwpTTmduNHdSSXlzeW5YSlBXaEhxQTg0VHBiek54ZC9XNGVsZGYzdERmZTlldkpUVkZ1REpNeGpWdFI3N0VHRTA0CnkyOGdZeUNXdGtXT2x4QnM4a2phaWRGa0FMNHNPbkVIL2pnOGN1M21rQk42QnpXRWpaMytXZnNvWnRqT0Q5VFMKWXVVVXh2cFBzRFdReXd2TmlKS1ZWcWJXKzhsbWpKNW11L2FjUEI1MXc1aVJpRTlscnB6VGV2dlZPd3JVUmN0Mgo2d2d1cnJyUkF0cndHZGVVZVNWWUxUVTNuS1pxUG8reElETm1IeTIyL3V1ZWtLSElKdUUyWDRuT1JDUzRyQ2lVClU0VXp2OWQxQklYV0dKVkRRdHBlOTlrQ2pRQmowWlhOR0dTNzRPd3JGWnU1ZElIcVdORnM5TFdIRThOWE4xankKTDRUaCtTcU5wbXhSMW5GdG5FM2dZbTNMdTVUVGl2NVh1WWpZLzVSaFNKMkxDRUpOSFd1cm5KZW0vZjdqcTBGagpaNEZsVEgzLzE0SjJLZFZCeFVhNXIrcWwrajhVYm5OSVlQNUZ6WHd6OHdlMWdiWllNVTM1azM1OStyUWdUMnh2CmJHVm5JRHh2Ykc5c2IyVm5RRzl0Ym1sd1lXTnJZV2RsTG05eVp6NkpBbTRFRXdFSUFGZ1dJUVNrb1Fjc0w1RDcKSDl1di9EeTZrZ0o0R3h2WUlBVUNhYjVTcmhzVWdBQUFBQUFFQUE1dFlXNTFNaXd5TGpVck1TNHhNaXd5TERJRApHeThFQlFzSkNBY0NBaUlDQmhVS0NRZ0xBZ1FXQWdNQkFoNEhBaGVBQUFvSkVMcVNBbmdiRzlnZ3Nlc1AvMHg0CjRoUDcraFVSUERXeTJPdWhCQk9VcU8rTGpwWGJRM24rUlRpb05Yekt2Q1hYdk1ucDhXczR1TDZJdEdwNmVydXUKdUlQdm81UWQ5ak9KMUh0UkJvVDg2b2tQeDVsNkU1OXVSaTBlRVdocXo2WkExSm5IZURHMFBjTmd1SE1aYVdOVAo4anRacjBucTRZaGlvb0RRN0NSUG1zMEptdGw4cHM1M3QxY0dSemRrM3RPbWxkcUtpbWp5YUp1L1NweExGMkMrCk9wTHdWREg1L3R3K3grZWthdG16SldqdmZVRVQyUzQzcHZaendGSUVaTEdNMEl6VkFORWRJUUxUMTE3OXkzM1kKSXRGRG0wQmpVN0F4bVR5NE9WZzFER0l5Qjk1RU51UmhZWGwvYXJ1N1BYVCtuWnVCTms3SXdJalpqckdFTXJpbgpRdXdaWDNVaStuK0hmQTJFMGxKdEZPQUpJRFZZM1lIY0xHamtYK3ZFYUt5RFhvUlJiVVZ4QzVuMU9QZDdCelN2CmphcE9EMHRxNk5BTW04QjAwZVVuTVNxUEN1dFdIUzRpbU1rZ2ZEK2xwajVaV3FIY3ZwVEU0UGFiTWU5OHA2akQKS3dpajRmRTBxYlhGYThkMnl4cFJTdk1yZGpQMHQydDNNQzgwNzltSWtqNWJjeEhIMUhtV1NzL0VtZmdFbWJEUQpBTllQSG1Vbmo3ZUZSYUlhc2h5NXBkTStMTVk3emZEWUgyUDkydUZBajE3a1lpdHY1LzN1bXBhSlRvV25leFp0Ckd5T3dFR2hlVU1uVklxTlJKM3BWbnNvVXkzdy9hOXRwY0JHSm04NHdVakhoL1pWaVY5RmVVOUZjR3F3c1JwRWUKZ1JZa2RzSFJOVzJ5ZnVQdU42UExaZkpvSW9wNWF5ZllqbU1Ob3dNZQo9N01iZQotLS0tLUVORCBQR1AgUFJJVkFURSBLRVkgQkxPQ0stLS0tLQo="
        provider: s3
        s3:
          bucket: repositories-test
          bucket_public_url: 'https://repositories-test.omnipackage.org'
          endpoint: 'https://example.r2.cloudflarestorage.com'
          access_key_id: 'key123'
          secret_access_key: 'secret123'
          region: auto
    "#,
        )
        .unwrap();

        let config = Config::load(&config_path).unwrap();

        assert_eq!(config.repositories.len(), 2);

        let localfs = &config.repositories[0];
        assert_eq!(localfs.name, "Local test");
        assert_eq!(localfs.provider, "localfs");
        assert_eq!(localfs.localfs().path, "/tmp/omnipackage-repos");
        assert!(localfs.s3.is_none());

        let s3 = &config.repositories[1];
        assert_eq!(s3.name, "CloudFlare R2");
        assert_eq!(s3.provider, "s3");
        assert_eq!(s3.s3().bucket, "repositories-test");
        assert_eq!(<std::option::Option<std::string::String> as Clone>::clone(&s3.s3().region).unwrap(), "auto");
        assert!(s3.localfs.is_none());
    }

    #[test]
    fn test_load_no_repositories() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yml");

        std::fs::write(
            &config_path,
            r#"
    extract_version:
      provider: file
      file:
        file: version.rb
        regex: VERSION
    builds: []
    "#,
        )
        .unwrap();

        let config = Config::load(&config_path).unwrap();
        assert!(config.repositories.is_empty());
    }
}
