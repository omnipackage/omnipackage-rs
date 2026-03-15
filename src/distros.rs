#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
pub struct Distro {
    pub id: String,
    pub name: String,
    pub image: String,
    pub arch: String,
    pub package_type: String,
    #[serde(default)]
    pub setup: Vec<String>,
    #[serde(default)]
    pub setup_repo: Vec<String>,
    #[serde(default)]
    pub install_steps: Vec<String>,
    pub image_info_url: Option<String>,
    pub deprecated: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
pub struct Distros {
    pub distros: Vec<Distro>,
}

static DISTROS: std::sync::OnceLock<Distros> = std::sync::OnceLock::new();

impl Distros {
    pub fn get() -> &'static Self {
        DISTROS.get_or_init(Self::load_default)
    }

    fn load_default() -> Self {
        Self::load(&Self::default_path()).expect("failed to load default distros")
    }

    pub fn load(path: &std::path::Path) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_saphyr::from_str(&content)?)
    }

    fn default_path() -> std::path::PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            let near_binary = exe.parent().unwrap_or(std::path::Path::new(".")).join("distros.yml");
            if near_binary.exists() {
                return near_binary;
            }
        }
        std::path::PathBuf::from("distros.yml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loads_successfully() {
        let distros = Distros::get();
        assert!(!distros.distros.is_empty());
    }

    #[test]
    fn test_distro_fields() {
        let distros = Distros::get();
        let opensuse = distros.distros.iter().find(|d| d.id == "opensuse_15.6").unwrap();

        assert_eq!(opensuse.image, "opensuse/leap:15.6");
        assert_eq!(opensuse.package_type, "rpm");
        assert_eq!(opensuse.arch, "x86_64");
        assert!(!opensuse.setup.is_empty());
        assert!(!opensuse.setup_repo.is_empty());
        assert!(!opensuse.install_steps.is_empty());
        assert!(opensuse.deprecated.is_none());
    }

    #[test]
    fn test_deprecated_field() {
        let distros = Distros::get();
        let deprecated = distros.distros.iter().find(|d| d.id == "debian_10").unwrap();
        assert!(deprecated.deprecated.is_some());

        let active = distros.distros.iter().find(|d| d.id == "debian_12").unwrap();
        assert!(active.deprecated.is_none());
    }

    #[test]
    fn test_merge_keys_resolved() {
        let distros = Distros::get();

        // these distros have no explicit setup, relies on anchor merge
        let fedora = distros.distros.iter().find(|d| d.id == "fedora_38").unwrap();
        assert!(!fedora.setup.is_empty());
        assert_eq!(fedora.package_type, "rpm");

        let ubuntu = distros.distros.iter().find(|d| d.id == "ubuntu_22.04").unwrap();
        assert!(!ubuntu.setup.is_empty());
        assert_eq!(ubuntu.package_type, "deb");
    }

    #[test]
    fn test_all_distros_have_required_fields() {
        let distros = Distros::get();
        for distro in &distros.distros {
            assert!(!distro.id.is_empty(), "distro {} has empty id", distro.id);
            assert!(!distro.name.is_empty(), "distro {} has empty name", distro.id);
            assert!(!distro.image.is_empty(), "distro {} has empty image", distro.id);
            assert!(!distro.package_type.is_empty(), "distro {} has empty package_type", distro.id);
            assert!(!distro.setup.is_empty(), "distro {} has empty setup", distro.id);
        }
    }
}
