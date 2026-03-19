#![allow(dead_code)]

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
        Self { exe: "gpg".to_string(), env: vec![] }
    }

    pub fn generate_keys(&self, name: &str, email: &str) -> Key {
        self.within_tmp_dir(|gpg, dir| {
            let batchfile_path = dir.join("genkey.batch");
            std::fs::write(&batchfile_path, self.batch_generate_keys(name, email)).expect("cannot write batchfile");

            gpg.cmd(["--no-tty", "--batch", "--gen-key", batchfile_path.to_str().unwrap()]).run().expect("gpg gen-key failed");

            let pub_key = gpg.export_key(name, false);
            let priv_key = gpg.export_key(name, true);
            Key { priv_key, pub_key }
        })
    }

    pub fn key_id(&self, key_string: &str) -> String {
        let key = key_string.to_string();
        self.cmd(["--show-keys"])
            .with_stdin(move |stdin| {
                stdin.write_all(key.as_bytes()).unwrap();
            })
            .capture()
            .expect("gpg --show-keys failed")
            .lines()
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_string()
    }

    pub fn key_info(&self, key_string: &str) -> String {
        let key = key_string.to_string();
        self.cmd(["--show-keys", "--with-fingerprint"])
            .with_stdin(move |stdin| {
                stdin.write_all(key.as_bytes()).unwrap();
            })
            .capture()
            .expect("gpg --show-keys failed")
    }

    pub fn test_key(&self, key: &Key) {
        self.within_tmp_dir(|gpg, _dir| {
            let import = |gpg: &Gpg, data: String| {
                gpg.cmd(["--import"])
                    .with_stdin(move |stdin| {
                        stdin.write_all(data.as_bytes()).unwrap();
                    })
                    .run()
                    .expect("gpg --import failed");
            };

            import(gpg, key.priv_key.clone());
            import(gpg, key.pub_key.clone());

            let data = "random string to encrypt".to_string();
            gpg.cmd(["-o", "/dev/null", "-as", "-"])
                .with_stdin(move |stdin| {
                    stdin.write_all(data.as_bytes()).unwrap();
                })
                .run()
                .expect("gpg sign test failed");
        });
    }

    fn export_key(&self, name: &str, secret: bool) -> String {
        let mut args = vec!["--armor".to_string()];
        if secret {
            args.push("--export-secret-keys".to_string())
        } else {
            args.push("--export".to_string())
        }
        args.push(name.to_string());

        self.cmd(args).capture().expect("gpg export failed")
    }

    fn cmd(&self, args: impl IntoIterator<Item = impl Into<String>>) -> crate::shell::Command {
        let mut cmd = Command::new(&self.exe);
        for (k, v) in &self.env {
            cmd = cmd.with_env(k, v);
        }
        cmd.args(args)
    }

    fn within_tmp_dir<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Self, PathBuf) -> R,
    {
        let dir = tempfile::tempdir().expect("cannot create tmp dir");
        std::fs::set_permissions(dir.path(), std::os::unix::fs::PermissionsExt::from_mode(0o700)).expect("cannot set permissions");

        let mut scoped = Self {
            exe: self.exe.clone(),
            env: vec![("GNUPGHOME".to_string(), dir.path().to_string_lossy().to_string())],
        };

        f(&mut scoped, dir.path().to_path_buf())
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
