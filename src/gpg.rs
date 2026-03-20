use crate::shell::Command;
use std::path::PathBuf;

pub struct Key {
    pub priv_key: String,
    pub pub_key: String,
}

pub struct Gpg {
    exe: String,
    env: Vec<(String, String)>,
}

impl Gpg {
    pub fn new() -> Self {
        Self {
            exe: "gpg".to_string(),
            env: vec![("GNUPGHOME".to_string(), "/dev/null".to_string()), ("GPG_AGENT_INFO".to_string(), "".to_string())],
        }
    }

    pub fn generate_keys(&self, name: &str, email: &str) -> Result<Key, String> {
        self.within_tmp_dir(|gpg, dir| {
            let batchfile_path = dir.join("genkey.batch");
            std::fs::write(&batchfile_path, self.batch_generate_keys(name, email)).map_err(|e| format!("cannot write batchfile: {}", e))?;

            gpg.cmd(["--no-tty", "--batch", "--gen-key", batchfile_path.to_str().unwrap()])
                .run()
                .map_err(|code| format!("gpg gen-key failed with exit code {}", code))?;

            let pub_key = gpg.export_key(name, false)?;
            let priv_key = gpg.export_key(name, true)?;
            Ok(Key { priv_key, pub_key })
        })
    }

    pub fn key_id(&self, key_string: &str) -> Result<String, String> {
        self.within_tmp_dir(|gpg, _dir| {
            let key = key_string.to_string();
            let output = gpg
                .cmd(["--show-keys"])
                .with_stdin(move |stdin| {
                    stdin.write_all(key.as_bytes()).unwrap();
                })
                .capture()
                .map_err(|code| format!("gpg --show-keys failed with exit code {}", code))?;

            Ok(output.lines().nth(1).unwrap_or("").trim().to_string())
        })
    }

    pub fn key_info(&self, key_string: &str) -> Result<String, String> {
        self.within_tmp_dir(|gpg, _dir| {
            let key = key_string.to_string();
            gpg.cmd(["--show-keys", "--with-fingerprint"])
                .with_stdin(move |stdin| {
                    stdin.write_all(key.as_bytes()).unwrap();
                })
                .capture()
                .map_err(|code| format!("gpg --show-keys failed with exit code {}", code))
        })
    }

    pub fn test_key(&self, key: &Key) -> Result<(), String> {
        self.within_tmp_dir(|gpg, _dir| {
            let import = |gpg: &Gpg, data: String| -> Result<(), String> {
                gpg.cmd(["--import"])
                    .with_stdin(move |stdin| {
                        stdin.write_all(data.as_bytes()).unwrap();
                    })
                    .run()
                    .map_err(|code| format!("gpg --import failed with exit code {}", code))
            };

            import(gpg, key.priv_key.clone())?;
            import(gpg, key.pub_key.clone())?;

            let data = "random string to encrypt".to_string();
            gpg.cmd(["-o", "/dev/null", "-as", "-"])
                .with_stdin(move |stdin| {
                    stdin.write_all(data.as_bytes()).unwrap();
                })
                .run()
                .map_err(|code| format!("gpg sign test failed with exit code {}", code))
        })
    }

    fn export_key(&self, name: &str, secret: bool) -> Result<String, String> {
        let mut args = vec!["--armor".to_string()];
        if secret {
            args.push("--export-secret-keys".to_string())
        } else {
            args.push("--export".to_string())
        }
        args.push(name.to_string());

        self.cmd(args).capture().map_err(|code| format!("gpg export failed with exit code {}", code))
    }

    fn cmd(&self, args: impl IntoIterator<Item = impl Into<String>>) -> Command {
        let mut cmd = Command::new(&self.exe);
        for (k, v) in &self.env {
            cmd = cmd.with_env(k, v);
        }
        cmd.args(args)
    }

    fn within_tmp_dir<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Self, PathBuf) -> R,
    {
        let dir = tempfile::tempdir().expect("cannot create tmp dir - aborting to prevent ~/.gnupg access");
        std::fs::set_permissions(dir.path(), std::os::unix::fs::PermissionsExt::from_mode(0o700)).expect("cannot set permissions on tmp dir - aborting to prevent ~/.gnupg access");

        let scoped = Self {
            exe: self.exe.clone(),
            env: vec![("GNUPGHOME".to_string(), dir.path().to_string_lossy().to_string()), ("GPG_AGENT_INFO".to_string(), "".to_string())],
        };

        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&scoped, dir.path().to_path_buf()))).unwrap_or_else(|e| {
            eprintln!("panic inside within_tmp_dir - aborting");
            std::panic::resume_unwind(e)
        })
    }

    fn batch_generate_keys(&self, name: &str, email: &str) -> String {
        format!(
            "Key-Type: RSA\n\
             Key-Length: 4096\n\
             Name-Real: {name}\n\
             Name-Email: {email}\n\
             Expire-Date: 0\n\
             %no-ask-passphrase\n\
             %no-protection\n\
             %commit\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpg_available() -> bool {
        std::process::Command::new("gpg")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn test_generate_keys() {
        if !gpg_available() {
            return;
        }
        let key = Gpg::new().generate_keys("Test User", "test@example.com").unwrap();

        assert!(key.priv_key.contains("-----BEGIN PGP PRIVATE KEY BLOCK-----"));
        assert!(key.priv_key.contains("-----END PGP PRIVATE KEY BLOCK-----"));
        assert!(key.pub_key.contains("-----BEGIN PGP PUBLIC KEY BLOCK-----"));
        assert!(key.pub_key.contains("-----END PGP PUBLIC KEY BLOCK-----"));
    }

    #[test]
    fn test_key_id() {
        if !gpg_available() {
            return;
        }
        let gpg = Gpg::new();
        let key = gpg.generate_keys("Test User", "test@example.com").unwrap();
        let id = gpg.key_id(&key.pub_key).unwrap();
        assert!(!id.is_empty());
    }

    #[test]
    fn test_key_info() {
        if !gpg_available() {
            return;
        }
        let gpg = Gpg::new();
        let key = gpg.generate_keys("Test User", "test@example.com").unwrap();
        let info = gpg.key_info(&key.pub_key).unwrap();
        assert!(info.contains("Test User"));
        assert!(info.contains("test@example.com"));
    }

    #[test]
    fn test_test_key_valid() {
        if !gpg_available() {
            return;
        }
        let gpg = Gpg::new();
        let key = gpg.generate_keys("Test User", "test@example.com").unwrap();
        assert!(gpg.test_key(&key).is_ok());
    }

    #[test]
    fn test_test_key_invalid() {
        if !gpg_available() {
            return;
        }
        let key = Key {
            priv_key: "invalid key".to_string(),
            pub_key: "invalid key".to_string(),
        };
        assert!(Gpg::new().test_key(&key).is_err());
    }
}
