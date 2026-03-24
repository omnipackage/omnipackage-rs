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

pub fn render(repositories: &Repositories) -> Result<String, String> {
    let html = PAGE_TEMPLATE_HTML;
    let start_tag = r#"<script type="application/json" id="data">"#;

    let start = html.find(start_tag).ok_or("cannot find data script tag")? + start_tag.len();
    let end = html[start..].find("</script>").ok_or("cannot find closing script tag")? + start;

    let json = serde_json::to_string_pretty(&repositories).map_err(|e| format!("cannot serialize data json: {}", e))?;

    Ok(format!("{}{}\n{}", &html[..start], json, &html[end..]))
}

pub fn upsert_by_distro_id(repositories: &mut Repositories, distro_id: impl Into<String>, data: Repository) {
    let distro_id = distro_id.into();

    if let Some(repo) = repositories.iter_mut().find(|repo| repo.get("distro_id").is_some_and(|value| value == &distro_id)) {
        repo.extend(data);
    } else {
        repositories.push(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string("tests/fixtures/install.html").expect("cannot read tests/fixtures/install.html")
    }

    #[test]
    fn test_parse() {
        let html = fixture();

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

    #[test]
    fn test_render_round_trip() {
        let html = fixture();
        let repos = parse(&html).expect("cannot parse");
        let injected = render(&repos).expect("cannot render");
        let repos2 = parse(&injected).expect("cannot parse after render");
        assert_eq!(repos, repos2);
    }

    /*#[test]
    fn test_render_preserves_html_structure() {
        let html = fixture();
        let repos = parse(&html).expect("cannot parse");
        let injected = render(&repos).expect("cannot render");

        // html outside the script tag is unchanged
        let start_tag = r#"<script type="application/json" id="data">"#;
        let before_original = &html[..html.find(start_tag).unwrap()];
        let before_injected = &injected[..injected.find(start_tag).unwrap()];
        assert_eq!(before_original, before_injected);

        let end_tag = "</script>";
        let after_original = &html[html.find(end_tag).unwrap()..];
        let after_injected = &injected[injected.find(end_tag).unwrap()..];
        assert_eq!(after_original, after_injected);
    }*/

    /*#[test]
    fn test_mutate_existing_entry() {
        let html = fixture();
        let mut repos = parse(&html).expect("cannot parse");

        repos.get_mut("debian_12").unwrap().distro_name = "Debian 12 LTS".to_string();

        upsert_by_distro_id(&repos, "debian_14", new_repo)

        let injected = render(&repos).expect("cannot render");
        let repos2 = parse(&injected).expect("cannot parse after render");

        assert_eq!(repos2["debian_12"].distro_name, "Debian 12 LTS");
        // other entries are untouched
        assert_eq!(repos2["debian_11"].distro_name, "Debian 11");
        assert_eq!(repos2.len(), 22);
    }*/

    #[test]
    fn test_mutate_add_entry() {
        let html = fixture();
        let mut repos = parse(&html).expect("cannot parse");

        let new_repo = Repository::from([
            ("distro_id".to_string(), "debian_14".to_string()),
            ("distro_name".to_string(), "Debian 14".to_string()),
            ("install_steps".to_string(), "apt install mpz".to_string()),
            ("gpg_key".to_string(), "pub rsa4096 2024-05-10 [SCEA]".to_string()),
            (
                "download_url".to_string(),
                "https://repositories.omnipackage.org/oleg/mpz/debian-14/stable/mpz_2.0.3-0_amd64.deb".to_string(),
            ),
        ]);
        upsert_by_distro_id(&mut repos, "debian_14", new_repo);

        let injected = render(&repos).expect("cannot render");
        let repos2 = parse(&injected).expect("cannot parse after render");

        assert_eq!(repos2.len(), 23);
        assert_eq!(
            repos2.iter().find(|repo| repo.get("distro_id").is_some_and(|v| v == "debian_14")).unwrap().get("distro_name").unwrap(),
            &"Debian 14".to_string()
        );
    }

    /*#[test]
    fn test_mutate_remove_entry() {
        let html = fixture();
        let mut repos = parse(&html).expect("cannot parse");

        repos.remove("debian_12");

        let injected = render(&repos).expect("cannot render");
        let repos2 = parse(&injected).expect("cannot parse after render");

        assert_eq!(repos2.len(), 21);
        assert!(!repos2.contains_key("debian_12"));
        // others still present
        assert!(repos2.contains_key("debian_11"));
        assert!(repos2.contains_key("debian_13"));
    }*/
}
