#![allow(dead_code)]

use crate::logger::Logger;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::io::{BufRead, Write};
use subprocess::{Exec, Redirection};

static CONTAINER_RUNTIME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn detect_container_runtime() -> String {
    let is_available = |program| {
        std::process::Command::new(program)
            .arg("info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    if is_available("podman") {
        "podman".to_string()
    } else if is_available("docker") {
        "docker".to_string()
    } else {
        panic!("neither podman nor docker found in $PATH")
    }
}

pub fn set_container_runtime(runtime: impl Into<String>) {
    CONTAINER_RUNTIME.set(runtime.into()).expect("container runtime already set");
}

fn container_runtime() -> &'static str {
    CONTAINER_RUNTIME.get_or_init(detect_container_runtime)
}

pub struct Command {
    program: String,
    args: Vec<String>,
    log_file: Option<std::path::PathBuf>,
    logger: Logger,
    stdin_fn: Option<Box<dyn FnOnce(&mut dyn std::io::Write)>>,
}

impl Command {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: vec![],
            log_file: None,
            logger: Logger::new(),
            stdin_fn: None,
        }
    }

    pub fn container(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            program: container_runtime().to_string(),
            args: args.into_iter().map(|a| a.into()).collect(),
            log_file: None,
            logger: Logger::new(),
            stdin_fn: None,
        }
    }

    pub fn with_stdin(mut self, f: impl FnOnce(&mut dyn std::io::Write) + 'static) -> Self {
        self.stdin_fn = Some(Box::new(f));
        self
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(|a| a.into()));
        self
    }

    pub fn log_to(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.log_file = Some(path.into());
        self
    }

    pub fn stream_output_to(mut self, logger: Logger) -> Self {
        self.logger = logger;
        self
    }

    pub fn run(self) -> std::result::Result<(), i32> {
        self.logger.cmd(&self.program, &self.args.join(" "));

        let mut log_file = self.log_file.as_ref().map(|path| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap_or_else(|e| panic!("cannot open log file {}: {}", path.display(), e))
        });

        let stdin_redirect = if self.stdin_fn.is_some() { Redirection::Pipe } else { Redirection::None };

        let mut job = Exec::cmd(&self.program)
            .args(&self.args)
            .stdin(stdin_redirect)
            .stdout(Redirection::Pipe)
            .stderr(Redirection::Merge)
            .start()
            .map_err(|e| {
                eprintln!("{}", e);
                1
            })?;

        if let Some(f) = self.stdin_fn {
            if let Some(mut stdin) = job.stdin.take() {
                f(&mut stdin);
            }
        }

        if let Some(stdout) = job.stdout.take() {
            for line in BufReader::new(stdout).lines().flatten() {
                let msg = self.logger.print(line);
                if let Some(ref mut file) = log_file {
                    writeln!(file, "{}", msg).ok();
                }
            }
        }

        let status = job.wait().map_err(|e| {
            eprintln!("{}", e);
            1
        })?;

        if status.success() { Ok(()) } else { Err(status.code().unwrap_or(1) as i32) }
    }
}
