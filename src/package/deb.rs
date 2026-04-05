use crate::build::job_variables::JobVariables;
use crate::config::{Build, Repository};
use crate::distros::Distro;
use crate::gpg::Key;
use crate::package::Package;
use crate::template::{Template, Var};
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Deb {
    pub distro: &'static Distro,
    pub source_dir: PathBuf,
    pub job_variables: JobVariables,
    pub distro_build_dir: PathBuf,

    mounts: HashMap<String, String>,
    commands: Vec<String>,
    build_output_dir: PathBuf,
    setup_stages: Vec<String>,
    gpgkey: Option<Key>,
}

impl Deb {
    pub fn new(distro: &'static Distro, source_dir: PathBuf, job_variables: JobVariables, distro_build_dir: PathBuf) -> Self {
        Self {
            distro,
            source_dir,
            job_variables,
            distro_build_dir: distro_build_dir.clone(),
            mounts: HashMap::new(),
            commands: Vec::new(),
            build_output_dir: distro_build_dir.clone(),
            setup_stages: Vec::new(),
            gpgkey: None,
        }
    }

    fn render_templates(&self, vars: HashMap<String, Var>, from: PathBuf, to: PathBuf) -> Result<(), Box<dyn Error>> {
        std::fs::create_dir_all(&to).map_err(|e| format!("cannot create directory {}: {}", to.display(), e))?;

        for entry in std::fs::read_dir(&from).map_err(|e| format!("cannot read dir {}: {}", from.display(), e))? {
            let path = entry?.path();
            let file_name = path.file_name().ok_or_else(|| format!("cannot get file name for {}", path.display()))?.to_string_lossy().into_owned();
            let dest = to.join(&file_name);

            if path.extension().and_then(|e| e.to_str()) == Some("liquid") {
                let stem = path.file_stem().ok_or_else(|| format!("cannot get file stem for {}", path.display()))?.to_string_lossy().into_owned();
                let dest_without_ext = to.join(stem);
                Template::from_file(&path)?.render_to_file(vars.clone(), dest_without_ext)?;
            } else {
                std::fs::copy(&path, &dest).map_err(|e| format!("cannot copy {} to {}: {}", path.display(), dest.display(), e))?;
            }
        }

        Ok(())
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

impl Package for Deb {
    fn setup_build(&mut self, config: Build) -> Result<(), Box<dyn Error>> {
        let debian_folder_template_path = config.deb.clone().ok_or("deb config is missing")?.debian_templates;

        let build_path = self.distro_build_dir().join("build");
        let output_path = self.distro_build_dir().join("output");
        std::fs::create_dir_all(&build_path).map_err(|e| format!("cannot create directory {}: {}", build_path.display(), e))?;
        std::fs::create_dir_all(&output_path).map_err(|e| format!("cannot create directory {}: {}", output_path.display(), e))?;

        let mut template_vars: HashMap<String, Var> = self.job_variables.to_template_vars();
        template_vars.extend(config.to_template_vars());
        self.render_templates(template_vars, self.source_dir.join(&debian_folder_template_path), build_path.join("debian"))?;

        self.mounts.insert(self.source_dir.to_string_lossy().to_string(), "/source".to_string());
        self.mounts.insert(build_path.to_string_lossy().to_string(), "/output/build".to_string());
        self.mounts.insert(output_path.to_string_lossy().to_string(), "/output/".to_string());

        self.commands.extend(self.distro.setup(&config.build_dependencies));
        if let Some(bbs) = self.before_build_script("/source", &config) {
            self.commands.push(bbs);
        }
        self.commands.extend([
            "cp -R /source/. /output/build/".to_string(),
            "cd /output/build".to_string(),
            "DEB_BUILD_OPTIONS=noddebs dpkg-buildpackage -b -tc".to_string(),
        ]);

        self.build_output_dir = output_path;
        self.setup_stages.push("build".to_string());

        Ok(())
    }

    fn setup_repository(&mut self, config: Repository) -> Result<(), Box<dyn Error>> {
        let gpgkey = self.prepare_gpgkey(&config)?;
        let (home_dir, repo_dir) = self.prepare_repository(&gpgkey)?;

        self.write_releases_script(&home_dir)?;

        self.mounts.insert(home_dir.to_string_lossy().to_string(), "/root".to_string());
        self.mounts.insert(repo_dir.to_string_lossy().to_string(), "/repo".to_string());

        self.commands.extend(self.distro.setup_repo.clone());
        self.commands.extend(self.import_gpg_keys_commands());
        self.commands.extend([
            "cd /repo".to_string(),
            "cp /output/*.deb /repo/".to_string(),
            "chmod +x /root/generate_releases_script.sh".to_string(),
            "mkdir -p stable".to_string(),
            "mv *.deb stable/".to_string(),
            "dpkg-scanpackages stable/ > stable/Packages".to_string(),
            "cat stable/Packages | gzip -1 > stable/Packages.gz".to_string(),
            "cd stable/".to_string(),
            "/root/generate_releases_script.sh > Release".to_string(),
            "gpg --no-tty --batch --yes --armor --detach-sign -o Release.gpg Release".to_string(),
            "gpg --no-tty --batch --yes --armor --detach-sign --clearsign -o InRelease Release".to_string(),
            "mv ../public.key Release.key".to_string(),
        ]);

        self.build_output_dir = repo_dir.clone();
        self.setup_stages.push("repository".to_string());
        self.gpgkey = Some(gpgkey);

        Ok(())
    }

    fn clone_box(&self) -> Box<dyn Package> {
        Box::new(self.clone())
    }

    fn source_dir(&self) -> PathBuf {
        self.source_dir.clone()
    }

    fn distro_build_dir(&self) -> PathBuf {
        self.distro_build_dir.clone()
    }

    fn distro(&self) -> &'static Distro {
        self.distro
    }

    fn mounts(&self) -> HashMap<String, String> {
        self.mounts.clone()
    }

    fn commands(&self) -> Vec<String> {
        self.commands.clone()
    }

    fn build_output_dir(&self) -> PathBuf {
        self.build_output_dir.clone()
    }

    fn setup_stages(&self) -> Vec<String> {
        self.setup_stages.clone()
    }

    fn gpgkey(&self) -> Option<Key> {
        self.gpgkey.clone()
    }
}
