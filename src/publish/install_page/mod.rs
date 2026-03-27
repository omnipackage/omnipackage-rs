use crate::distros::Distros;
use crate::template::{Template, Var};
use std::collections::HashMap;
use std::error::Error;

const PAGE_TEMPLATE_HTML: &str = include_str!("install.html.liquid");
const BADGE_TEMPLATE_SVG: &str = include_str!("badge.svg.liquid");

pub type Repository = HashMap<String, String>;
pub type Repositories = Vec<Repository>;

#[derive(Debug, Clone)]
pub struct Output {
    pub install_page: String,
    pub badge: String,
}

pub fn upsert(existing_page_html: &str, repositories: &Repositories, template_vars: HashMap<String, Var>) -> Result<Output, Box<dyn Error>> {
    let mut repos = parse(existing_page_html).unwrap_or_else(|_| vec![]);

    repositories.iter().for_each(|repo| {
        upsert_repository(&mut repos, repo.clone());
    });

    let template_html = render(&repos)?;

    let install_page = Template::from_content(&template_html).render(template_vars.clone());
    let badge = render_badge(&repos, template_vars.clone());
    Ok(Output { install_page, badge })
}

fn render_badge(repositories: &Repositories, template_vars: HashMap<String, Var>) -> String {
    let mut vars = template_vars.clone();

    let (rpm_count, deb_count) = repositories.iter().fold((0, 0), |(rpm, deb), r| match r.get("package_type").map(|t| t.as_str()) {
        Some("rpm") => (rpm + 1, deb),
        Some("deb") => (rpm, deb + 1),
        _ => (rpm, deb),
    });

    vars.insert("title".to_string(), "OmniPackage repositories badge".into());
    vars.insert("text".to_string(), format!("{rpm_count} RPM {deb_count} DEB").into());

    Template::from_content(BADGE_TEMPLATE_SVG).render(vars)
}

fn parse(html: &str) -> Result<Repositories, Box<dyn Error>> {
    let (start, end) = extract_json_bounds(html)?;
    let json = html[start..end].trim();
    Ok(serde_json::from_str(json)?)
}

fn render(repositories: &Repositories) -> Result<String, Box<dyn Error>> {
    let html = PAGE_TEMPLATE_HTML;
    let (start_pos, end_pos) = extract_json_bounds(html)?;

    let ids = Distros::get().ids();
    let mut sorted = repositories.clone();
    sorted.sort_by_key(|repo| repo.get("distro_id").and_then(|id| ids.iter().position(|d| d == id)).unwrap_or(usize::MAX));
    let json = serde_json::to_string_pretty(&sorted)?;

    let mut rendered = String::with_capacity(html.len() + json.len());
    rendered.push_str(&html[..start_pos]);
    rendered.push_str(&json);
    rendered.push_str(&html[end_pos..]);

    Ok(rendered)
}

fn extract_json_bounds(html: &str) -> Result<(usize, usize), Box<dyn Error>> {
    let start_tag = r#"<script type="application/json" id="data">"#;
    let start = html.find(start_tag).ok_or("cannot find data script tag")? + start_tag.len();
    let end = html[start..].find("</script>").ok_or("cannot find closing script tag")? + start;
    Ok((start, end))
}

fn upsert_repository(repositories: &mut Repositories, data: Repository) {
    let distro_id = data.get("distro_id").unwrap();

    if let Some(repo) = repositories.iter_mut().find(|repo| repo.get("distro_id").is_some_and(|value| value == distro_id)) {
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
        assert_eq!(err.to_string(), "cannot find data script tag");
    }

    #[test]
    fn test_parse_missing_closing_tag() {
        let html = r#"<script type="application/json" id="data">[{"key": "value"}]"#;
        let err = parse(html).unwrap_err();
        assert_eq!(err.to_string(), "cannot find closing script tag");
    }

    #[test]
    fn test_parse_invalid_json() {
        let html = r#"<script type="application/json" id="data">not json</script>"#;
        let err = parse(html).unwrap_err();
        assert_eq!(err.to_string(), "expected ident at line 1 column 2");
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
        let mut repos = parse(&html).expect("cannot parse");
        repos.sort_by_key(|r| r.get("distro_id").cloned().unwrap_or_default());
        let injected = render(&repos).expect("cannot render");
        let mut repos2 = parse(&injected).expect("cannot parse after render");
        repos2.sort_by_key(|r| r.get("distro_id").cloned().unwrap_or_default());
        assert_eq!(repos, repos2);
    }

    #[test]
    fn test_upsert_repository() {
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
        upsert_repository(&mut repos, new_repo);

        let injected = render(&repos).expect("cannot render");
        let repos2 = parse(&injected).expect("cannot parse after render");

        assert_eq!(repos2.len(), 23);
        assert_eq!(
            repos2.iter().find(|repo| repo.get("distro_id").is_some_and(|v| v == "debian_14")).unwrap().get("distro_name").unwrap(),
            &"Debian 14".to_string()
        );
    }

    #[test]
    fn test_mutate_upsert_all() {
        let html = fixture();
        let mut repos = parse(&html).expect("cannot parse");

        let new_repo1 = Repository::from([
            ("distro_id".to_string(), "debian_14".to_string()),
            ("distro_name".to_string(), "Debian 14".to_string()),
            ("install_steps".to_string(), "apt install mpz".to_string()),
            ("gpg_key".to_string(), "pub rsa4096 2024-05-10 [SCEA]".to_string()),
            (
                "download_url".to_string(),
                "https://repositories.omnipackage.org/oleg/mpz/debian-14/stable/mpz_2.0.3-0_amd64.deb".to_string(),
            ),
            ("package_type".to_string(), "rpm".into()),
        ]);
        let new_repo2 = Repository::from([
            ("distro_id".to_string(), "debian_15".to_string()),
            ("distro_name".to_string(), "Debian 15 LTS".to_string()),
            ("install_steps".to_string(), "apt install mpz".to_string()),
            ("gpg_key".to_string(), "pub rsa4096 2024-05-10 [SCEA]".to_string()),
            (
                "download_url".to_string(),
                "https://repositories.omnipackage.org/oleg/mpz/debian-15/stable/mpz_2.0.3-0_amd64.deb".to_string(),
            ),
            ("package_type".to_string(), "deb".into()),
        ]);

        let vars: HashMap<String, Var> = HashMap::from([
            ("package_name".to_string(), "testpackage".to_string().into()),
            ("description".to_string(), "testpackage description".to_string().into()),
            ("homepage".to_string(), "http://testpacka.ge".to_string().into()),
        ]);
        let new_repos: Repositories = vec![new_repo1, new_repo2];
        let result = upsert(&html, &new_repos, vars).unwrap();

        assert!(result.install_page.contains("Debian 14"));
        assert!(result.install_page.contains("Debian 15 LTS"));
        assert!(result.install_page.contains("http://testpacka.ge"));

        assert!(result.badge.contains("1 RPM 1 DEB"));

        let repos2 = parse(&result.install_page).unwrap();
        assert_eq!(repos2.len(), 24);
    }
}
