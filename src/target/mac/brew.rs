use crate::LoggingArgs;
use crate::shell::Command;
use anyhow::Result;
use std::path::{Path, PathBuf};

const ENV: [(&str, &str); 4] = [
    ("HOMEBREW_NO_AUTO_UPDATE", "1"),
    ("HOMEBREW_NO_ANALYTICS", "1"),
    ("HOMEBREW_NO_INSTALL_CLEANUP", "1"),
    ("HOMEBREW_NO_ENV_HINTS", "1"),
];

pub struct Brew {
    logging: LoggingArgs,
    log: PathBuf,
}

impl Brew {
    pub fn new(logging: LoggingArgs, log: PathBuf) -> Self {
        Self { logging, log }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        ENV.iter().fold(Command::new("brew"), |c, (k, v)| c.with_env(*k, *v)).args(args.iter().map(|a| a.to_string()))
    }

    pub fn run(&self, args: &[&str]) -> Result<()> {
        self.cmd(args).stream_output_to(self.logging.container_logger()).log_to(&self.log).run()
    }

    pub fn run_in(&self, dir: &Path, args: &[&str]) -> Result<()> {
        self.cmd(args).current_dir(dir).stream_output_to(self.logging.container_logger()).log_to(&self.log).run()
    }

    pub fn capture(&self, args: &[&str]) -> Result<String> {
        self.cmd(args).capture()
    }

    pub fn tap_remote(&self, tap: &str) -> Result<Option<String>> {
        let repo = PathBuf::from(self.capture(&["--repository", tap])?.trim());
        if !repo.join(".git").exists() {
            return Ok(None);
        }
        let remote = Command::new("git").arg("-C").arg(repo.to_string_lossy().into_owned()).args(["remote", "get-url", "origin"]).capture()?;
        Ok(Some(remote.trim().to_string()))
    }

    pub fn developer_mode(&self) -> Result<bool> {
        Ok(self.capture(&["developer"])?.contains("enabled"))
    }
}
