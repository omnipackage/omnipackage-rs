use crate::build::BuildContext;
use crate::build::job_variables::JobVariables;
use crate::build::package::Package;
use crate::config::Build;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

impl BuildContext {
    pub fn setup_deb(&self) -> Package {
        let debian_folder_template_path = self.config.deb.clone().unwrap().debian_templates;
        // debian_folder = ::OmnipackageAgent::Build::Deb::DebianFolder.new(::OmnipackageAgent::Utils::Path.mkpath(source_path, debian_folder_template_path))

        let build_folder_name = format!("{}-{}", self.config.package_name, self.distro.id);

        let build_path = self.build_dir.join(&build_folder_name).join("build");
        let output_path = self.build_dir.join(&build_folder_name).join("output");
        std::fs::create_dir_all(&build_path).unwrap_or_else(|e| panic!("cannot create directory {}: {}", build_path.display(), e));
        std::fs::create_dir_all(&output_path).unwrap_or_else(|e| panic!("cannot create directory {}: {}", output_path.display(), e));

        //template_params = build_conf.merge(job_variables)
        //debian_folder.save(::OmnipackageAgent::Utils::Path.mkpath(build_path, 'debian'), template_params)

        let mut mounts: HashMap<String, String> = HashMap::new();
        mounts.insert(self.source_path.to_string_lossy().to_string(), "/source".to_string());
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

        Package { mounts, commands, output_path }
    }
}
