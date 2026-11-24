use regex::Regex;

pub fn quarantine_steps(cask: &str) -> Vec<String> {
    let re = Regex::new(r#"(?m)^\s*app\s+"([^"]+)"(?:\s*,\s*target:\s*"([^"]+)")?"#).unwrap();
    re.captures_iter(cask)
        .map(|c| {
            let name = c.get(2).or(c.get(1)).unwrap().as_str();
            let path = if name.starts_with('/') { name.to_string() } else { format!("/Applications/{name}") };
            format!("xattr -dr com.apple.quarantine \"{path}\"")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarantine_steps_follow_app_stanzas() {
        let cask =
            "cask \"x\" do\n  app \"My App.app\"\n  app \"Other.app\", target: \"Renamed.app\"\n  app \"Abs.app\", target: \"/Users/Shared/Abs.app\"\n  # app \"Commented.app\" is not a stanza\nend\n";
        assert_eq!(
            quarantine_steps(cask),
            vec![
                "xattr -dr com.apple.quarantine \"/Applications/My App.app\"",
                "xattr -dr com.apple.quarantine \"/Applications/Renamed.app\"",
                "xattr -dr com.apple.quarantine \"/Users/Shared/Abs.app\"",
            ]
        );
        assert!(quarantine_steps("cask \"x\" do\n  pkg \"x.pkg\"\nend\n").is_empty());
    }
}
