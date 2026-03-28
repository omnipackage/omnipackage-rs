use liquid::ParserBuilder;
use liquid::model::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Var(Value);

impl From<String> for Var {
    fn from(s: String) -> Self {
        Var(Value::scalar(s))
    }
}

impl From<&str> for Var {
    fn from(s: &str) -> Self {
        Var(Value::scalar(s.to_string()))
    }
}

impl From<i64> for Var {
    fn from(i: i64) -> Self {
        Var(Value::scalar(i))
    }
}

impl From<bool> for Var {
    fn from(b: bool) -> Self {
        Var(Value::scalar(b))
    }
}

impl From<Vec<String>> for Var {
    fn from(v: Vec<String>) -> Self {
        Var(Value::Array(v.into_iter().map(Value::scalar).collect()))
    }
}

impl From<HashMap<String, String>> for Var {
    fn from(map: HashMap<String, String>) -> Self {
        let obj: liquid::Object = map.into_iter().map(|(k, v)| (k.into(), Value::scalar(v))).collect();
        Var(Value::Object(obj))
    }
}

pub struct Template {
    template: liquid::Template,
}

impl Template {
    pub fn from_content(content: impl Into<String>) -> Self {
        let template = ParserBuilder::with_stdlib()
            .build()
            .unwrap()
            .parse(&content.into())
            .unwrap_or_else(|e| panic!("cannot parse template: {}", e));
        Self { template }
    }

    pub fn from_file(path: impl AsRef<Path>) -> Self {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read template {}: {}", path.as_ref().display(), e));
        Self::from_content(content)
    }

    pub fn render(&self, vars: impl IntoIterator<Item = (String, Var)>) -> String {
        let mut globals: liquid::Object = vars.into_iter().map(|(k, v)| (k.into(), v.0)).collect();

        // Retry until all unknown variables are resolved, filling each with "".
        loop {
            match self.template.render(&globals) {
                Ok(output) => return output,
                Err(e) => {
                    let msg = e.to_string();
                    if let Some(var_name) = msg.lines().find_map(|line| line.trim().strip_prefix("requested variable=")).map(|s| s.trim().to_owned()) {
                        globals.insert(var_name.into(), Value::scalar(""));
                    } else {
                        panic!("cannot render template: {}", e);
                    }
                }
            }
        }
    }

    pub fn render_to_file(&self, vars: impl IntoIterator<Item = (String, Var)>, output_path: PathBuf) {
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("cannot create directory {}: {}", parent.display(), e));
        }

        let output = self.render(vars);
        std::fs::write(&output_path, output).unwrap_or_else(|e| panic!("cannot write to {}: {}", output_path.display(), e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_template(content: &str) -> (Template, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.liquid");
        std::fs::write(&path, content).unwrap();
        (Template::from_file(path), dir)
    }

    #[test]
    fn test_render_strings() {
        let (template, _dir) = make_template("Hello, {{ name }}! Version: {{ version }}");
        let output = template.render([("name".to_string(), "world".into()), ("version".to_string(), "1.2.3".into())]);
        assert_eq!(output, "Hello, world! Version: 1.2.3");
    }

    #[test]
    fn test_render_integer() {
        let (template, _dir) = make_template("Port: {{ port }}");
        let output = template.render([("port".to_string(), 8080i64.into())]);
        assert_eq!(output, "Port: 8080");
    }

    #[test]
    fn test_render_bool() {
        let (template, _dir) = make_template("{% if debug %}DEBUG{% endif %}");
        let output = template.render([("debug".to_string(), true.into())]);
        assert_eq!(output, "DEBUG");
    }

    #[test]
    fn test_render_vec() {
        let (template, _dir) = make_template("{% for item in items %}{{ item }} {% endfor %}");
        let output = template.render([("items".to_string(), vec!["foo".to_string(), "bar".to_string(), "baz".to_string()].into())]);
        assert_eq!(output, "foo bar baz ");
    }

    #[test]
    fn test_render_to_file() {
        let (template, dir) = make_template("Hello, {{ name }}!");
        let output_path = dir.path().join("output.txt");

        template.render_to_file([("name".to_string(), "world".into())], output_path.clone());

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert_eq!(content, "Hello, world!");
    }

    #[test]
    fn test_render_to_file_creates_directories() {
        let (template, _dir) = make_template("Hello, {{ name }}!");
        let dir = tempfile::tempdir().unwrap();
        let output_path = dir.path().join("nested/dirs/output.txt");

        template.render_to_file([("name".to_string(), "world".into())], output_path.clone());

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert_eq!(content, "Hello, world!");
    }

    #[test]
    fn test_render_unknown_variable_falls_back_to_empty_string() {
        let (template, _dir) = make_template("Hello, {{ name }}! Extra: {{ CMAKE_EXTRA_CLI }}");
        let output = template.render([("name".to_string(), "world".into())]);
        assert_eq!(output, "Hello, world! Extra: ");
    }
}
