use crate::build::package::template::Var;
use chrono::Utc;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct JobVariables {
    pub version: String,
    pub current_time_rfc2822: String,
}

impl JobVariables {
    pub fn build(version: String) -> Self {
        JobVariables {
            version,
            current_time_rfc2822: Utc::now().to_rfc2822(),
        }
    }

    pub fn to_vars(&self) -> HashMap<String, Var> {
        let mut vars = HashMap::new();
        vars.insert("version".to_string(), self.version.clone().into());
        vars.insert("current_time_rfc2822".to_string(), self.current_time_rfc2822.clone().into());
        vars
    }
}

impl std::fmt::Display for JobVariables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "version={} current_time_rfc2822={}", self.version, self.current_time_rfc2822)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_variables_build() {
        let vars = JobVariables::build("1.2.3".to_string());

        assert_eq!(vars.version, "1.2.3");
        assert!(!vars.current_time_rfc2822.is_empty());

        // verify it's valid RFC 2822 by parsing it back
        assert!(chrono::DateTime::parse_from_rfc2822(&vars.current_time_rfc2822).is_ok());

        let re = regex::Regex::new(r"^\w{3}, \d{1,2} \w{3} \d{4} \d{2}:\d{2}:\d{2} [+-]\d{4}$").unwrap();
        assert!(re.is_match(&vars.current_time_rfc2822), "unexpected format: {}", vars.current_time_rfc2822);
    }

    #[test]
    fn test_display() {
        let vars = JobVariables {
            version: "3.2.1".to_string(),
            current_time_rfc2822: "Mon, 1 Jan 2024 12:00:00 +0000".to_string(),
        };
        assert_eq!(vars.to_string(), "version=3.2.1 current_time_rfc2822=Mon, 1 Jan 2024 12:00:00 +0000");
    }
}
