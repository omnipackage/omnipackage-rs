use crate::gpg::Key;
use crate::publish::PublishContext;
use std::error::Error;
use std::path::{Path, PathBuf};

impl PublishContext {
    pub fn setup_deb_repo(&self, key: &Key, home_dir: &Path, work_dir: &Path) -> Result<(), Box<dyn Error>> {
        self.write_releases_script(home_dir)?;

        let mut commands = self.import_gpg_keys_commands();
        commands.extend([
            "chmod +x /root/generate_releases_script.sh".to_string(),
            "mkdir -p stable".to_string(),
            "mv *.deb stable/".to_string(),
            "dpkg-scanpackages stable/ > stable/Packages".to_string(),
            "cat stable/Packages | gzip -9 > stable/Packages.gz".to_string(),
            "cd stable/".to_string(),
            "/root/generate_releases_script.sh > Release".to_string(),
            "gpg --no-tty --batch --yes --armor --detach-sign -o Release.gpg Release".to_string(),
            "gpg --no-tty --batch --yes --armor --detach-sign --clearsign -o InRelease Release".to_string(),
            "mv ../public.key Release.key".to_string(),
        ]);

        self.execute(commands, home_dir, work_dir)
    }

    fn write_releases_script(&self, home_dir: &Path) -> Result<(), Box<dyn Error>> {
        // credit: https://earthly.dev/blog/creating-and-hosting-your-own-deb-packages-and-apt-repo/
        let script = r#"#!/bin/sh
set -e

do_hash() {
    HASH_NAME=$1
    HASH_CMD=$2
    echo "${HASH_NAME}:"
    for f in $(find -type f); do
        f=$(echo $f | cut -c3-) # remove ./ prefix
        if [ "$f" = "Release" ]; then
            continue
        fi
        echo " $(${HASH_CMD} ${f}  | cut -d" " -f1) $(wc -c $f)"
    done
}

cat << EOF
Origin: Omnipackage repository
Label: Example
Suite: stable
Codename: stable
Version: 1.0
Architectures: amd64
Components: main
Description: Omnipackage repository
Date: $(date -Ru)
EOF
do_hash "MD5Sum" "md5sum"
do_hash "SHA1" "sha1sum"
do_hash "SHA256" "sha256sum"
"#;

        Ok(std::fs::write(home_dir.join("generate_releases_script.sh"), script)?)
    }
}
