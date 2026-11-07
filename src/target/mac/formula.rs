use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BottleMeta {
    pub tag: String,
    pub filename: String,
    pub local_filename: String,
    pub sha256: String,
    pub cellar: String,
    pub rebuild: u32,
}

// port of Homebrew's Formulary.class_s
pub fn formula_class(name: &str) -> String {
    let mut chars = name.chars();
    let capitalized: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    };
    let camel = regex::Regex::new(r"[-_.\s]([a-zA-Z0-9])").unwrap().replace_all(&capitalized, |c: &regex::Captures| c[1].to_uppercase());
    let plus = camel.replace('+', "x");
    regex::Regex::new(r"(.)@(\d)").unwrap().replace(&plus, "${1}AT${2}").into_owned()
}

pub fn parse_brew_bottle_json(json: &str) -> Result<Vec<BottleMeta>> {
    let root: serde_json::Value = serde_json::from_str(json).context("cannot parse brew bottle json")?;
    let mut metas = vec![];
    for formula in root.as_object().context("brew bottle json is not an object")?.values() {
        let bottle = &formula["bottle"];
        let cellar = bottle["cellar"].as_str().context("bottle json has no cellar")?.to_string();
        let rebuild = bottle["rebuild"].as_u64().unwrap_or(0) as u32;
        for (tag, entry) in bottle["tags"].as_object().context("bottle json has no tags")? {
            let field = |name: &str| entry[name].as_str().map(str::to_string).with_context(|| format!("bottle json tag {tag} has no {name}"));
            metas.push(BottleMeta {
                tag: tag.clone(),
                filename: field("filename")?,
                local_filename: field("local_filename")?,
                sha256: field("sha256")?,
                cellar: cellar.clone(),
                rebuild,
            });
        }
    }
    Ok(metas)
}

pub fn bottle_block(root_url: &str, bottles: &[BottleMeta]) -> String {
    let mut sorted = bottles.to_vec();
    sorted.sort_by(|a, b| a.tag.cmp(&b.tag));

    let mut block = format!("  bottle do\n    root_url \"{root_url}\"\n");
    if let Some(rebuild) = sorted.first().map(|b| b.rebuild).filter(|r| *r > 0) {
        block.push_str(&format!("    rebuild {rebuild}\n"));
    }
    for b in &sorted {
        let cellar = match b.cellar.as_str() {
            "any" | "any_skip_relocation" => format!(":{}", b.cellar),
            path => format!("\"{path}\""),
        };
        block.push_str(&format!("    sha256 cellar: {cellar}, {}: \"{}\"\n", b.tag, b.sha256));
    }
    block.push_str("  end\n");
    block
}

pub fn insert_bottle(formula: &str, source_sha256: &str, block: &str) -> Result<String> {
    let marker = format!("sha256 \"{source_sha256}\"");
    let mut out = String::with_capacity(formula.len() + block.len() + 1);
    let mut inserted = false;
    for line in formula.split_inclusive('\n') {
        out.push_str(line);
        if !inserted && line.trim_start().starts_with(&marker) {
            if !line.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
            out.push_str(block);
            inserted = true;
        }
    }
    if !inserted {
        anyhow::bail!("formula template must contain the line `sha256 \"{{{{ sha256 }}}}\"` so the bottle block can be placed after it");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_names_match_homebrew() {
        assert_eq!(formula_class("ophello"), "Ophello");
        assert_eq!(formula_class("my-app"), "MyApp");
        assert_eq!(formula_class("foo_bar.baz"), "FooBarBaz");
        assert_eq!(formula_class("foo@1"), "FooAT1");
        assert_eq!(formula_class("libc++"), "Libcxx");
        assert_eq!(formula_class("MPZ"), "Mpz");
    }

    fn meta(tag: &str, cellar: &str, rebuild: u32) -> BottleMeta {
        BottleMeta {
            tag: tag.to_string(),
            filename: format!("hello-1.0.{tag}.bottle.tar.gz"),
            local_filename: format!("hello--1.0.{tag}.bottle.tar.gz"),
            sha256: format!("{tag}-sha"),
            cellar: cellar.to_string(),
            rebuild,
        }
    }

    #[test]
    fn bottle_block_sorts_tags_and_formats_cellars() {
        let block = bottle_block("https://r.example/macos/bottles", &[meta("sequoia", "/usr/local/Cellar", 0), meta("arm64_sequoia", "any", 0)]);
        assert_eq!(
            block,
            "  bottle do\n    root_url \"https://r.example/macos/bottles\"\n    sha256 cellar: :any, arm64_sequoia: \"arm64_sequoia-sha\"\n    sha256 cellar: \"/usr/local/Cellar\", sequoia: \"sequoia-sha\"\n  end\n"
        );
    }

    #[test]
    fn bottle_block_emits_rebuild() {
        let block = bottle_block("u", &[meta("arm64_tahoe", "any_skip_relocation", 2)]);
        assert!(block.contains("    rebuild 2\n"));
        assert!(block.contains("cellar: :any_skip_relocation, arm64_tahoe:"));
    }

    #[test]
    fn inserts_bottle_after_source_sha() {
        let formula = "class Hello < Formula\n  url \"u\"\n  sha256 \"abc\"\n\n  def install\n  end\nend\n";
        let out = insert_bottle(formula, "abc", "  bottle do\n  end\n").unwrap();
        assert_eq!(out, "class Hello < Formula\n  url \"u\"\n  sha256 \"abc\"\n\n  bottle do\n  end\n\n  def install\n  end\nend\n");
    }

    #[test]
    fn insert_bottle_ignores_sha_mentioned_in_comments() {
        let formula = "# keep sha256 \"abc\"\nclass Hello < Formula\n  sha256 \"abc\"\nend\n";
        let out = insert_bottle(formula, "abc", "  bottle do\n  end\n").unwrap();
        assert_eq!(out, "# keep sha256 \"abc\"\nclass Hello < Formula\n  sha256 \"abc\"\n\n  bottle do\n  end\nend\n");
    }

    #[test]
    fn insert_bottle_requires_sha_line() {
        let err = insert_bottle("class Hello < Formula\nend\n", "abc", "b").unwrap_err();
        assert!(err.to_string().contains("sha256"));
    }

    #[test]
    fn parses_brew_bottle_json() {
        let json = r#"{"omnipackage/hello/hello":{"formula":{"name":"hello","pkg_version":"1.0"},"bottle":{"root_url":"file:///o","cellar":"any","rebuild":0,"tags":{"arm64_sequoia":{"filename":"hello-1.0.arm64_sequoia.bottle.tar.gz","local_filename":"hello--1.0.arm64_sequoia.bottle.tar.gz","sha256":"deadbeef"}}}}}"#;
        let metas = parse_brew_bottle_json(json).unwrap();
        assert_eq!(
            metas,
            vec![BottleMeta {
                tag: "arm64_sequoia".to_string(),
                filename: "hello-1.0.arm64_sequoia.bottle.tar.gz".to_string(),
                local_filename: "hello--1.0.arm64_sequoia.bottle.tar.gz".to_string(),
                sha256: "deadbeef".to_string(),
                cellar: "any".to_string(),
                rebuild: 0,
            }]
        );
    }
}
