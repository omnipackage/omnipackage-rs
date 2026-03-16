use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Clone)]
pub struct Distros {
    distros: Vec<Distro>,
}

const DISTROS_YAML: &str = include_str!("../distros.yml");
static DISTROS: std::sync::OnceLock<Distros> = std::sync::OnceLock::new();

#[allow(dead_code)]
impl Distros {
    pub fn get() -> &'static Self {
        DISTROS.get_or_init(Self::load_default)
    }

    pub fn load(path: &std::path::Path) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_saphyr::from_str(&content)?)
    }

    fn load_default() -> Self {
        serde_saphyr::from_str(DISTROS_YAML).expect("failed to parse embedded distros.yml")
    }

    pub fn iter(&self) -> impl Iterator<Item = &Distro> {
        self.distros.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.distros.is_empty()
    }

    pub fn by_id(&self, id: &str) -> &Distro {
        self.iter().find(|d| d.id == id).unwrap_or_else(|| panic!("distro '{}' not found", id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loads_successfully() {
        let distros = Distros::get();
        assert!(!distros.is_empty());
    }

    #[test]
    fn test_distro_fields() {
        let distros = Distros::get();
        let opensuse = distros.iter().find(|d| d.id == "opensuse_15.6").unwrap();

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
        let deprecated = distros.iter().find(|d| d.id == "debian_10").unwrap();
        assert!(deprecated.deprecated.is_some());

        let active = distros.iter().find(|d| d.id == "debian_12").unwrap();
        assert!(active.deprecated.is_none());
    }

    #[test]
    fn test_merge_keys_resolved() {
        let distros = Distros::get();

        // these distros have no explicit setup, relies on anchor merge
        let fedora = distros.iter().find(|d| d.id == "fedora_38").unwrap();
        assert!(!fedora.setup.is_empty());
        assert_eq!(fedora.package_type, "rpm");

        let ubuntu = distros.iter().find(|d| d.id == "ubuntu_22.04").unwrap();
        assert!(!ubuntu.setup.is_empty());
        assert_eq!(ubuntu.package_type, "deb");
    }

    #[test]
    fn test_all_distros_have_required_fields() {
        let distros = Distros::get();
        for distro in distros.iter() {
            assert!(!distro.id.is_empty(), "distro {} has empty id", distro.id);
            assert!(!distro.name.is_empty(), "distro {} has empty name", distro.id);
            assert!(!distro.image.is_empty(), "distro {} has empty image", distro.id);
            assert!(!distro.package_type.is_empty(), "distro {} has empty package_type", distro.id);
            assert!(!distro.setup.is_empty(), "distro {} has empty setup", distro.id);
        }
    }

    #[test]
    fn test_by_id_found() {
        let distros = Distros::get();
        let distro = distros.by_id("opensuse_15.6");
        assert_eq!(distro.id, "opensuse_15.6");
    }

    #[test]
    fn test_by_id_not_found() {
        let distros = Distros::get();
        let result = std::panic::catch_unwind(|| distros.by_id("nonexistent"));
        assert!(result.is_err());
    }
}
