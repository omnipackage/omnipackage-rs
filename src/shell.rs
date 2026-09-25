use crate::logger::Logger;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use subprocess::{Exec, Redirection};

static CONTAINER_RUNTIME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn detect_container_runtime() -> String {
    if let Ok(runtime) = std::env::var("OMNIPACKAGE_CONTAINER_RUNTIME") {
        return runtime;
    }

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

const HOST_RUNTIME: &str = "host";

pub fn is_host_runtime() -> bool {
    CONTAINER_RUNTIME
        .get()
        .cloned()
        .or_else(|| std::env::var("OMNIPACKAGE_CONTAINER_RUNTIME").ok())
        .is_some_and(|runtime| runtime == HOST_RUNTIME)
}

type StdinFn = Box<dyn FnOnce(&mut dyn std::io::Write)>;

#[derive(Debug)]
struct ExitError(i32);

impl std::fmt::Display for ExitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "process exited with code {}", self.0)
    }
}

impl Error for ExitError {}

pub struct Command {
    program: String,
    args: Vec<String>,
    log_file: Option<PathBuf>,
    logger: Logger,
    stdin_fn: Option<StdinFn>,
    env_vars: Vec<(String, String)>,
    current_dir: Option<PathBuf>,
}

impl Command {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: vec![],
            log_file: None,
            logger: Logger::new(),
            stdin_fn: None,
            env_vars: vec![],
            current_dir: None,
        }
    }

    pub fn container(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            program: container_runtime().to_string(),
            args: args.into_iter().map(|a| a.into()).collect(),
            log_file: None,
            logger: Logger::new(),
            stdin_fn: None,
            env_vars: vec![],
            current_dir: None,
        }
    }

    pub fn current_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(path.into());
        self
    }

    pub fn with_stdin(mut self, f: impl FnOnce(&mut dyn std::io::Write) + 'static) -> Self {
        self.stdin_fn = Some(Box::new(f));
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.push((key.into(), value.into()));
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

    fn build_exec(&self) -> Exec {
        let stdin_redirect = if self.stdin_fn.is_some() { Redirection::Pipe } else { Redirection::None };

        let mut exec = Exec::cmd(&self.program).args(&self.args).stdin(stdin_redirect).stdout(Redirection::Pipe).stderr(Redirection::Merge);

        if let Some(ref dir) = self.current_dir {
            exec = exec.cwd(dir);
        }

        for (k, v) in &self.env_vars {
            exec = exec.env(k, v);
        }

        exec
    }

    fn feed_stdin(stdin_fn: Option<StdinFn>, job: &mut subprocess::Job) {
        if let Some(f) = stdin_fn
            && let Some(mut stdin) = job.stdin.take()
        {
            f(&mut stdin);
        }
    }

    pub fn run(self) -> Result<(), anyhow::Error> {
        if self.program == HOST_RUNTIME {
            let (cmd, _links) = self.into_host()?;
            return cmd.run();
        }

        self.logger.cmd(&self.program, &self.args, &self.env_vars);

        let mut log_file = self.log_file.as_ref().map(|path| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap_or_else(|e| panic!("cannot open log file {}: {}", path.display(), e))
        });

        let mut job = self.build_exec().start()?;

        Self::feed_stdin(self.stdin_fn, &mut job);

        if let Some(stdout) = job.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let msg = self.logger.print(line);
                if let Some(ref mut file) = log_file {
                    writeln!(file, "{}", msg).ok();
                }
            }
        }

        let status = job.wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(ExitError(status.code().unwrap_or(1) as i32)))
        }
    }

    pub fn capture(self) -> Result<String, anyhow::Error> {
        self.logger.cmd(&self.program, &self.args, &self.env_vars);

        let mut job = self.build_exec().start()?;

        Self::feed_stdin(self.stdin_fn, &mut job);

        let mut output = String::new();
        if let Some(stdout) = job.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                output.push_str(&line);
                output.push('\n');
            }
        }

        let status = job.wait()?;
        if status.success() {
            Ok(output)
        } else {
            Err(anyhow::anyhow!(ExitError(status.code().unwrap_or(1) as i32)))
        }
    }

    pub fn run_interactive(self) -> Result<(), anyhow::Error> {
        if self.program == HOST_RUNTIME {
            let (cmd, _links) = self.into_host()?;
            return cmd.run_interactive();
        }

        self.logger.cmd(&self.program, &self.args, &self.env_vars);

        let mut cmd = std::process::Command::new(&self.program);
        cmd.args(&self.args);
        for (k, v) in &self.env_vars {
            cmd.env(k, v);
        }

        if let Some(ref dir) = self.current_dir {
            cmd.current_dir(dir);
        }

        let status = cmd.status()?;
        if status.success() { Ok(()) } else { Err(anyhow::anyhow!(ExitError(status.code().unwrap_or(1)))) }
    }

    fn into_host(self) -> Result<(Command, Symlinks), anyhow::Error> {
        let mut args = self.args.into_iter();
        anyhow::ensure!(args.next().as_deref() == Some("run"), "host runtime supports only \"run\"");

        let mut program = None;
        let mut env_vars = self.env_vars;
        let mut links = Symlinks(vec![]);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--rm" | "-it" | "--pull=never" => {}
                "--entrypoint" => program = args.next(),
                "-e" => {
                    let var = args.next().unwrap_or_default();
                    let (k, v) = var.split_once('=').with_context(|| format!("invalid env var {var}"))?;
                    env_vars.push((k.to_string(), v.to_string()));
                }
                "--mount" => links.add(&args.next().unwrap_or_default())?,
                _ if arg.starts_with('-') => anyhow::bail!("host runtime does not support {arg}"),
                _ => break,
            }
        }

        let cmd = Command {
            program: program.context("host runtime requires --entrypoint")?,
            args: args.collect(),
            log_file: self.log_file,
            logger: self.logger,
            stdin_fn: self.stdin_fn,
            env_vars,
            current_dir: self.current_dir,
        };
        Ok((cmd, links))
    }
}

struct Symlinks(Vec<PathBuf>);

impl Symlinks {
    fn add(&mut self, mount: &str) -> Result<(), anyhow::Error> {
        let opts: HashMap<&str, &str> = mount.split(',').filter_map(|kv| kv.split_once('=')).collect();
        let (Some(source), Some(target)) = (opts.get("source"), opts.get("target")) else {
            anyhow::bail!("invalid mount {mount}");
        };

        let link = PathBuf::from(target.trim_end_matches('/'));
        if link.is_symlink() {
            std::fs::remove_file(&link)?;
        }
        let source = std::fs::canonicalize(source).with_context(|| format!("cannot resolve mount source {source}"))?;
        std::os::unix::fs::symlink(&source, &link).with_context(|| format!("cannot link {} to {}", link.display(), source.display()))?;
        self.0.push(link);
        Ok(())
    }
}

impl Drop for Symlinks {
    fn drop(&mut self) {
        for link in &self.0 {
            let _ = std::fs::remove_file(link);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_real_command_succeeds() {
        Command::new("echo").args(["hello"]).run().unwrap();
    }

    #[test]
    fn test_run_real_command_fails() {
        let result = Command::new("false").run();
        assert!(result.is_err());
    }

    #[test]
    fn test_capture_returns_stdout() {
        let output = Command::new("echo").args(["hello"]).capture().unwrap();
        assert_eq!(output.trim(), "hello");
    }

    #[test]
    fn test_with_env_passes_env_var() {
        let output = Command::new("sh").args(["-c", "echo $MY_VAR"]).with_env("MY_VAR", "hello").capture().unwrap();
        assert_eq!(output.trim(), "hello");
    }

    #[test]
    fn test_with_stdin_passes_input() {
        let output = Command::new("cat")
            .with_stdin(|stdin| {
                stdin.write_all(b"hello").unwrap();
            })
            .capture()
            .unwrap();
        assert_eq!(output.trim(), "hello");
    }

    #[test]
    fn test_log_to_writes_output() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("test.log");

        Command::new("echo").args(["logged"]).log_to(&log).run().unwrap();

        let content = std::fs::read_to_string(&log).unwrap();
        assert!(content.contains("logged"));
    }

    #[test]
    fn test_container_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-runtime");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let _ = set_container_runtime(script.to_string_lossy().to_string());
    }

    #[test]
    fn test_host_run_links_mounts_and_passes_env() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("file"), "").unwrap();
        let target = dir.path().join("mnt");

        Command::new(HOST_RUNTIME)
            .args([
                "run".to_string(),
                "--rm".to_string(),
                "--entrypoint".to_string(),
                "/bin/sh".to_string(),
                "--mount".to_string(),
                format!("type=bind,source={},target={}/", source.display(), target.display()),
                "-e".to_string(),
                "FOO=bar".to_string(),
                "image".to_string(),
                "-c".to_string(),
                format!("test -f {}/file && test \"$FOO\" = bar", target.display()),
            ])
            .run()
            .unwrap();

        assert!(!target.is_symlink());
    }

    #[test]
    fn test_host_run_rejects_other_commands() {
        assert!(Command::new(HOST_RUNTIME).args(["login"]).run().is_err());
    }
}
