use crate::distros::Distro;
use crate::logger::{Color, Logger, colorize};
use std::fmt;
use std::path::PathBuf;

pub struct Output {
    pub distro: &'static Distro,
    pub success: bool,
    pub build_log: PathBuf,
    pub artefacts: Vec<PathBuf>,
}

impl fmt::Display for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.success {
            write!(
                f,
                "{}: {}",
                self.distro.name,
                self.artefacts.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
            )
        } else {
            write!(f, "{}: {}", self.distro.name, self.build_log.display())
        }
    }
}

pub fn log_all(outputs: &Vec<Output>) {
    let (succeeded, failed): (Vec<_>, Vec<_>) = outputs.iter().partition(|o| o.success);
    let mut summary = String::new();
    if !succeeded.is_empty() {
        summary.push_str(&format!("{}:\n", colorize(Color::BoldGreen, "succeeded")));
        for o in &succeeded {
            summary.push_str(&format!("  {}\n", o));
        }
    }
    if !failed.is_empty() {
        summary.push_str(&format!("{}:\n", colorize(Color::BoldRed, "failed")));
        for o in &failed {
            summary.push_str(&format!("  {}\n", o));
        }
    }

    Logger::new().info(format!("all build results\n{}", summary));
}
