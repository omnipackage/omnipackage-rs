use std::collections::HashMap;

const PAGE_TEMPLATE_HTML: &str = include_str!("install.html.liquid");

pub type Repository = HashMap<String, String>;
pub type Repositories = Vec<Repository>;

pub fn parse(html: &str) -> Result<Repositories, String> {
    let start_tag = r#"<script type="application/json" id="data">"#;

    let start = html.find(start_tag).ok_or("cannot find data script tag")? + start_tag.len();
    let end = html[start..].find("</script>").ok_or("cannot find closing script tag")? + start;
    let json = html[start..end].trim();

    serde_json::from_str(json).map_err(|e| format!("cannot parse data json: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        let html = std::fs::read_to_string("tests/fixtures/install.html").expect("cannot read tests/fixtures/install.html");

        let entries = parse(&html).expect("cannot extract data");

        assert_eq!(entries.len(), 22);

        let first = &entries[0];
        assert_eq!(first["distro_id"], "opensuse_15.5");
        assert_eq!(first["distro_name"], "openSUSE 15.5");
        assert_eq!(first["download_url"], "https://repositories.omnipackage.org/oleg/mpz/opensuse-15-5/mpz-2.0.3-1.x86_64.rpm");
        assert_eq!(
            first["install_steps"],
            "zypper addrepo --refresh https://repositories.omnipackage.org/oleg/mpz/opensuse-15-5/mpz.repo\nzypper refresh\nzypper install mpz"
        );

        let last = &entries[21];
        assert_eq!(last["distro_id"], "mageia_cauldron");
        assert_eq!(last["distro_name"], "Mageia Cauldron");
        assert_eq!(last["download_url"], "https://repositories.omnipackage.org/oleg/mpz/mageia-cauldron/mpz-2.0.3-1.mga10.x86_64.rpm");

        // assert all entries have the required keys
        let required_keys = ["distro_id", "distro_name", "install_steps", "gpg_key", "download_url"];
        for entry in &entries {
            for key in &required_keys {
                assert!(entry.contains_key(*key), "entry '{}' missing key '{key}'", entry["distro_id"]);
                assert!(!entry[*key].is_empty(), "entry '{}' has empty key '{key}'", entry["distro_id"]);
            }
        }

        // assert all distro_ids are unique
        let mut ids: Vec<&str> = entries.iter().map(|e| e["distro_id"].as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), entries.len(), "distro_ids are not unique");

        // assert all download_urls end with .rpm or .deb
        for entry in &entries {
            let url = &entry["download_url"];
            assert!(url.ends_with(".rpm") || url.ends_with(".deb"), "entry '{}' has unexpected download_url: {url}", entry["distro_id"]);
        }
    }

    #[test]
    fn test_parse_missing_script_tag() {
        let html = "<html><body><p>no data here</p></body></html>";
        let err = parse(html).unwrap_err();
        assert_eq!(err, "cannot find data script tag");
    }

    #[test]
    fn test_parse_missing_closing_tag() {
        let html = r#"<script type="application/json" id="data">[{"key": "value"}]"#;
        let err = parse(html).unwrap_err();
        assert_eq!(err, "cannot find closing script tag");
    }

    #[test]
    fn test_parse_invalid_json() {
        let html = r#"<script type="application/json" id="data">not json</script>"#;
        let err = parse(html).unwrap_err();
        assert!(err.starts_with("cannot parse data json:"), "unexpected error: {err}");
    }

    #[test]
    fn test_parse_empty_array() {
        let html = r#"<script type="application/json" id="data">[]</script>"#;
        let entries = parse(html).expect("should parse empty array");
        assert!(entries.is_empty());
    }
}
