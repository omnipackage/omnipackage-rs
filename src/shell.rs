use crate::logger;

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
    inner: std::process::Command,
}

impl Command {
    pub fn new(program: impl AsRef<std::ffi::OsStr>) -> Self {
        Self {
            inner: std::process::Command::new(program),
        }
    }

    pub fn container(args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>) -> Self {
        let mut cmd = Self::new(container_runtime());
        cmd.inner.args(args);
        cmd
    }

    pub fn arg(mut self, arg: impl AsRef<std::ffi::OsStr>) -> Self {
        self.inner.arg(arg);
        self
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>) -> Self {
        self.inner.args(args);
        self
    }

    pub fn run(mut self) -> std::result::Result<(), i32> {
        let program = self.inner.get_program().to_string_lossy().to_string();
        let args = self.inner.get_args().map(|a| a.to_string_lossy().to_string()).collect::<Vec<_>>().join(" ");

        logger::cmd(&program, &args);

        match self.inner.status() {
            Ok(status) => {
                let code = status.code().unwrap_or(1);
                if code == 0 { Ok(()) } else { Err(code) }
            }
            Err(e) => {
                eprintln!("{}", e);
                Err(1)
            }
        }
    }
}
