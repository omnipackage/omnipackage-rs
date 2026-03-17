use crate::build::job_variables::JobVariables;
use crate::build::package::template::{Template, Var};
use crate::build::package::{PackageInput, PackageOutput};
use crate::config::Build;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

impl PackageInput {
    pub fn setup_rpm(&self) -> PackageOutput {
        let specfile_path_template_path = self.build_context.build_config.rpm.clone().unwrap().spec_template;

        let rpmbuild_folder_name = format!("{}-{}", self.build_context.build_config.package_name, self.distro.id);
        let rpmbuild_path = self.build_context.build_dir.join(&rpmbuild_folder_name);
        std::fs::create_dir_all(&rpmbuild_path).unwrap_or_else(|e| panic!("cannot create directory {}: {}", rpmbuild_path.display(), e));
        // @output_path = rpmbuild_path

        let source_folder_name = format!("{}-{}", self.build_context.build_config.package_name, self.job_variables.version);
        let specfile_name = format!("{}-{}.spec", source_folder_name, self.distro.id);

        let mut vars: HashMap<String, Var> = self.build_context.job_variables.to_vars();
        vars.extend(self.build_context.build_config.to_vars());
        vars.insert("source_folder_name".to_string(), source_folder_name.into());
        let template = Template::new(self.build_context.source_path.join(&specfile_path_template_path));
        template.render_to_file(vars, self.build_context.build_dir.join(&rpmbuild_folder_name).join(&specfile_name));

        PackageOutput {
            mounts: HashMap::new(),
            commands: Vec::new(),
            source_path: self.build_context.source_path.clone(),
            output_path: rpmbuild_path.clone(),
        }
    }
}
