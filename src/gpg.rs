#![allow(dead_code)]

use crate::shell::Command;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub struct Key {
    pub priv_key: String,
    pub pub_key: String,
}

pub struct Gpg {
    exe: String,
}

impl Gpg {
    pub fn new() -> Self {
        Self { exe: "gpg".to_string() }
    }

    pub fn with_exe(exe: impl Into<String>) -> Self {
        Self { exe: exe.into() }
    }

    pub fn generate_keys(&self, name: &str, email: &str) -> Key {
        self.within_tmp_dir(|dir, gnupghome| {
            let batchfile_path = dir.join("genkey.batch");
            std::fs::write(&batchfile_path, self.batch_generate_keys(name, email)).expect("cannot write batchfile");

            Command::new(&self.exe)
                .args(["--no-tty", "--batch", "--gen-key", batchfile_path.to_str().unwrap()])
                .with_env("GNUPGHOME", &gnupghome)
                .run()
                .expect("gpg gen-key failed");

            let pub_key = self.export_key(&gnupghome, name, false);
            let priv_key = self.export_key(&gnupghome, name, true);

            Key { priv_key, pub_key }
        })
    }

    pub fn key_id(&self, key_string: &str) -> String {
        let key = key_string.to_string();
        let output = Command::new(&self.exe)
            .arg("--show-keys")
            .with_stdin(move |stdin| {
                stdin.write_all(key.as_bytes()).unwrap();
            })
            .capture()
            .expect("gpg --show-keys failed");

        output.lines().nth(1).unwrap_or("").trim().to_string()
    }

    pub fn key_info(&self, key_string: &str) -> String {
        let key = key_string.to_string();
        crate::shell::Command::new(&self.exe)
            .args(["--show-keys", "--with-fingerprint"])
            .with_stdin(move |stdin| {
                stdin.write_all(key.as_bytes()).unwrap();
            })
            .capture()
            .expect("gpg --show-keys failed")
    }

    pub fn test_key(&self, key: &Key) {
        self.within_tmp_dir(|_dir, gnupghome| {
            let import = |data: &str| {
                let data = data.to_string();
                Command::new(&self.exe)
                    .arg("--import")
                    .with_env("GNUPGHOME", &gnupghome)
                    .with_stdin(move |stdin| {
                        stdin.write_all(data.as_bytes()).unwrap();
                    })
                    .run()
                    .expect("gpg --import failed");
            };

            import(&key.priv_key);
            import(&key.pub_key);

            Command::new(&self.exe)
                .args(["-o", "/dev/null", "-as", "-"])
                .with_env("GNUPGHOME", &gnupghome)
                .with_stdin(|stdin| {
                    stdin.write_all(b"random string to encrypt").unwrap();
                })
                .run()
                .expect("gpg sign test failed");
        });
    }

    fn export_key(&self, gnupghome: &str, name: &str, secret: bool) -> String {
        let mut args = vec!["--armor"];
        if secret {
            args.push("--export-secret-keys")
        } else {
            args.push("--export")
        }
        args.push(name);

        Command::new(&self.exe).args(args).with_env("GNUPGHOME", gnupghome).capture().expect("gpg export failed")
    }

    fn within_tmp_dir<F, R>(&self, f: F) -> R
    where
        F: FnOnce(PathBuf, String) -> R,
    {
        let dir = tempfile::tempdir().expect("cannot create tmp dir");

        // set permissions to 0700 as required by gpg
        std::fs::set_permissions(dir.path(), PermissionsExt::from_mode(0o700)).expect("cannot set permissions on tmp dir");

        let gnupghome = dir.path().to_string_lossy().to_string();
        f(dir.path().to_path_buf(), gnupghome)
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
