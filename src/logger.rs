#![allow(dead_code)]

struct Config {
    colors: bool,
    stderr: bool,
}

static CONFIG: std::sync::OnceLock<Config> = std::sync::OnceLock::new();

pub fn set_stderr(value: bool) {
    CONFIG.get_or_init(|| Config {
        colors: std::env::var("NO_COLOR").is_err(),
        stderr: value,
    });
}

fn config() -> &'static Config {
    CONFIG.get_or_init(|| Config {
        colors: std::env::var("NO_COLOR").is_err(),
        stderr: false,
    })
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();

    let secs = now.as_secs();
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

#[derive(Clone, Copy)]
pub enum Color {
    Red,
    Green,
    Yellow,
    Cyan,
    Bold,
    BoldRed,
    BoldGreen,
    BoldYellow,
    BoldCyan,
}

impl Color {
    fn code(self) -> &'static str {
        match self {
            Color::Red => "\x1b[31m",
            Color::Green => "\x1b[32m",
            Color::Yellow => "\x1b[33m",
            Color::Cyan => "\x1b[36m",
            Color::Bold => "\x1b[1m",
            Color::BoldRed => "\x1b[1;31m",
            Color::BoldGreen => "\x1b[1;32m",
            Color::BoldYellow => "\x1b[1;33m",
            Color::BoldCyan => "\x1b[1;36m",
        }
    }
}

fn colorize_with(colors: bool, color: Color, text: impl std::fmt::Display) -> String {
    if colors { format!("{}{}\x1b[0m", color.code(), text) } else { format!("{}", text) }
}

pub fn colorize(color: Color, text: impl std::fmt::Display) -> String {
    colorize_with(config().colors, color, text)
}

fn print(msg: String) {
    if config().stderr {
        eprintln!("{}", msg);
    } else {
        println!("{}", msg);
    }
}

pub fn info(msg: impl std::fmt::Display) {
    print(format!("{} {} {}", colorize(Color::Cyan, timestamp()), colorize(Color::Green, "[INFO]"), msg));
}

pub fn warn(msg: impl std::fmt::Display) {
    print(format!("{} {} {}", colorize(Color::Cyan, timestamp()), colorize(Color::Yellow, "[WARN]"), msg));
}

pub fn error(msg: impl std::fmt::Display) {
    print(format!("{} {} {}", colorize(Color::Cyan, timestamp()), colorize(Color::Red, "[ERROR]"), msg));
}

pub fn cmd(program: &str, args: &str) {
    print(format!(
        "{} {} {}",
        colorize(Color::Cyan, timestamp()),
        colorize(Color::Cyan, "$"),
        colorize(Color::Bold, format!("{} {}", program, args))
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colorize_disabled() {
        assert_eq!(colorize_with(false, Color::Red, "hello"), "hello");
    }

    #[test]
    fn test_colorize_enabled() {
        let result = colorize_with(true, Color::Red, "hello");
        assert!(result.contains("\x1b[31m"));
        assert!(result.contains("hello"));
        assert!(result.ends_with("\x1b[0m"));
    }

    #[test]
    fn test_timestamp_format() {
        let ts = timestamp();
        assert_eq!(ts.len(), 8);
        assert_eq!(ts.chars().nth(2), Some(':'));
        assert_eq!(ts.chars().nth(5), Some(':'));
    }
}
