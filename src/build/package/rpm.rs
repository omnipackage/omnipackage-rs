use crate::build::BuildContext;
use crate::build::package::Package;
use crate::template::{Template, Var};
use std::collections::HashMap;
use std::error::Error;

impl BuildContext {
    pub fn setup_rpm(&self) -> Result<Package, Box<dyn Error>> {
        let specfile_path_template_path = self.config.rpm.clone().ok_or("rpm config is missing")?.spec_template;

        let rpmbuild_path = self.distro_build_dir();
        std::fs::create_dir_all(&rpmbuild_path).map_err(|e| format!("cannot create directory {}: {}", rpmbuild_path.display(), e))?;

        let source_folder_name = format!("{}-{}", self.config.package_name, self.job_variables.version);
        let specfile_name = format!("{}-{}.spec", source_folder_name, self.distro.id);

        let mut template_vars: HashMap<String, Var> = self.job_variables.to_template_vars();
        template_vars.extend(self.config.to_template_vars());
        template_vars.insert("source_folder_name".to_string(), source_folder_name.clone().into());
        let template = Template::from_file(self.source_dir.join(&specfile_path_template_path))?;
        template.render_to_file(template_vars, rpmbuild_path.join(&specfile_name))?;

        let mut mounts: HashMap<String, String> = HashMap::new();
        mounts.insert(self.source_dir.to_string_lossy().to_string(), "/source".to_string());
        mounts.insert(rpmbuild_path.to_string_lossy().to_string(), "/root/rpmbuild".to_string());

        let mut commands: Vec<String> = self.distro.setup(&self.config.build_dependencies);
        if let Some(bbs) = self.before_build_script("/source") {
            commands.push(bbs);
        }
        commands.extend([
            "rpmdev-setuptree".to_string(),
            "rm -rf /root/rpmbuild/SOURCES/*".to_string(),
            format!("cp -R /source /root/rpmbuild/SOURCES/{source_folder_name}"),
            "cd /root/rpmbuild/SOURCES/".to_string(),
            format!("tar -cvzf {source_folder_name}.tar.gz --exclude='.git' --exclude='node_modules' {source_folder_name}/"),
            format!("cd /root/rpmbuild/SOURCES/{source_folder_name}/"),
            format!("QA_RPATHS=$(( 0x0001|0x0010|0x0002|0x0004|0x0008|0x0020 )) rpmbuild --clean -bb /root/rpmbuild/{specfile_name}"),
        ]);

        Ok(Package {
            distro: self.distro,
            mounts,
            commands,
            output_path: rpmbuild_path.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LoggingArgs;
    use crate::build::job_variables::JobVariables;
    use crate::config::{Build, RpmConfig};
    use crate::distros::Distro;

    fn make_distro() -> Distro {
        Distro {
            id: "opensuse_15.6".to_string(),
            name: "openSUSE Leap 15.6".to_string(),
            image: "opensuse/leap:15.6".to_string(),
            arch: "x86_64".to_string(),
            package_type: "rpm".to_string(),
            setup: vec!["zypper install -y %{build_dependencies}".to_string()],
            setup_repo: vec![],
            install_steps: vec![],
            image_info_url: None,
            deprecated: None,
        }
    }

    fn make_build_config() -> Build {
        Build {
            distro: "opensuse_15.6".to_string(),
            package_name: "myapp".to_string(),
            maintainer: "Test <test@test.com>".to_string(),
            homepage: "https://example.com".to_string(),
            description: "Test package".to_string(),
            build_dependencies: vec!["gcc".to_string(), "make".to_string()],
            runtime_dependencies: vec![],
            before_build_script: None,
            rpm: Some(RpmConfig {
                spec_template: ".omnipackage/specfile.spec.liquid".to_string(),
            }),
            deb: None,
            rest: HashMap::new(),
        }
    }

    #[test]
    fn test_setup_rpm() {
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().to_path_buf();
        let build_dir = tempfile::tempdir().unwrap();

        // create spec template
        let spec_dir = source_dir.join(".omnipackage");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("specfile.spec.liquid"), "Name: {{ package_name }}\nVersion: {{ version }}").unwrap();

        let distro = Box::new(make_distro());
        let distro_ref: &'static Distro = Box::leak(distro);

        let context = BuildContext {
            distro: distro_ref,
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

        let package = context.setup_rpm().unwrap();

        // verify mounts
        assert!(package.mounts.contains_key(&source_dir.to_string_lossy().to_string()));
        assert!(package.mounts.values().any(|v| v == "/source"));
        assert!(package.mounts.values().any(|v| v == "/root/rpmbuild"));

        // verify commands contain expected steps
        let cmds = package.commands.join(" ");
        assert!(cmds.contains("zypper install"));
        assert!(cmds.contains("rpmdev-setuptree"));
        assert!(cmds.contains("rpmbuild"));
        assert!(cmds.contains("myapp-1.2.3"));

        // verify specfile was rendered
        let specfile = build_dir.path().join("myapp-opensuse_15.6").join("myapp-1.2.3-opensuse_15.6.spec");
        assert!(specfile.exists());
        let content = std::fs::read_to_string(&specfile).unwrap();
        assert!(content.contains("myapp"));
        assert!(content.contains("1.2.3"));
    }
}
