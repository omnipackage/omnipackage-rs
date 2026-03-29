use crate::build::BuildContext;
use crate::build::package::Package;
use crate::template::{Template, Var};
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;

impl BuildContext {
    pub fn setup_deb(&self) -> Result<Package, Box<dyn Error>> {
        let debian_folder_template_path = self.config.deb.clone().ok_or("deb config is missing")?.debian_templates;

        let build_path = self.distro_build_dir().join("build");
        let output_path = self.distro_build_dir().join("output");
        std::fs::create_dir_all(&build_path).map_err(|e| format!("cannot create directory {}: {}", build_path.display(), e))?;
        std::fs::create_dir_all(&output_path).map_err(|e| format!("cannot create directory {}: {}", output_path.display(), e))?;

        let mut template_vars: HashMap<String, Var> = self.job_variables.to_template_vars();
        template_vars.extend(self.config.to_template_vars());
        self.render_templates(template_vars, self.source_dir.join(&debian_folder_template_path), build_path.join("debian"))?;

        let mut mounts: HashMap<String, String> = HashMap::new();
        mounts.insert(self.source_dir.to_string_lossy().to_string(), "/source".to_string());
        mounts.insert(build_path.to_string_lossy().to_string(), "/output/build".to_string());
        mounts.insert(output_path.to_string_lossy().to_string(), "/output/".to_string());

        let mut commands: Vec<String> = self.distro.setup(&self.config.build_dependencies);
        if let Some(bbs) = self.before_build_script("/source") {
            commands.push(bbs);
        }
        commands.extend([
            "cp -R /source/. /output/build/".to_string(),
            "cd /output/build".to_string(),
            "DEB_BUILD_OPTIONS=noddebs dpkg-buildpackage -b -tc".to_string(),
        ]);

        Ok(Package {
            distro: self.distro,
            mounts,
            commands,
            output_path,
        })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LoggingArgs;
    use crate::build::job_variables::JobVariables;
    use crate::config::{Build, DebConfig};
    use crate::distros::Distro;

    fn make_distro() -> Distro {
        Distro {
            id: "debian_12".to_string(),
            name: "Debian 12".to_string(),
            image: "debian:12".to_string(),
            arch: "x86_64".to_string(),
            package_type: "deb".to_string(),
            setup: vec!["apt-get install -y %{build_dependencies}".to_string()],
            setup_repo: vec![],
            install_steps: vec![],
            image_info_url: None,
            deprecated: None,
        }
    }

    fn make_build_config() -> Build {
        Build {
            distro: "debian_12".to_string(),
            package_name: "myapp".to_string(),
            maintainer: "Test <test@test.com>".to_string(),
            homepage: "https://example.com".to_string(),
            description: "Test package".to_string(),
            build_dependencies: vec!["gcc".to_string(), "make".to_string()],
            runtime_dependencies: vec!["libc6".to_string()],
            before_build_script: None,
            rpm: None,
            deb: Some(DebConfig {
                debian_templates: ".omnipackage/deb".to_string(),
            }),
            rest: HashMap::new(),
        }
    }

    #[test]
    fn test_setup_deb() {
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().to_path_buf();
        let build_dir = tempfile::tempdir().unwrap();

        // create debian templates
        let deb_dir = source_dir.join(".omnipackage/deb");
        std::fs::create_dir_all(&deb_dir).unwrap();
        std::fs::write(deb_dir.join("control.liquid"), "Package: {{ package_name }}\nVersion: {{ version }}").unwrap();
        std::fs::write(deb_dir.join("rules"), "#!/usr/bin/make -f\n%:\n\tdh $@").unwrap();

        let distro = Box::leak(Box::new(make_distro()));

        let context = BuildContext {
            distro,
            source_dir: source_dir.clone(),
            config: make_build_config(),
            job_variables: JobVariables::build("1.2.3".to_string()),
            build_dir: build_dir.path().to_path_buf(),
            logging_args: LoggingArgs {
                container_output: "null".to_string(),
                disable_container_echo: false,
                fail_log_lines: 42,
            },
        };

        let package = context.setup_deb().unwrap();

        // verify mounts
        assert!(package.mounts.values().any(|v| v == "/source"));
        assert!(package.mounts.values().any(|v| v == "/output/build"));
        assert!(package.mounts.values().any(|v| v == "/output/"));

        // verify commands
        let cmds = package.commands.join(" ");
        assert!(cmds.contains("apt-get install"));
        assert!(cmds.contains("dpkg-buildpackage"));

        // verify liquid template was rendered
        let control = build_dir.path().join("myapp-debian_12/build/debian/control");
        assert!(control.exists());
        let content = std::fs::read_to_string(&control).unwrap();
        assert!(content.contains("myapp"));
        assert!(content.contains("1.2.3"));

        // verify plain file was copied
        let rules = build_dir.path().join("myapp-debian_12/build/debian/rules");
        assert!(rules.exists());
    }
}
