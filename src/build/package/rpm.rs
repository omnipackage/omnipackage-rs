use crate::build::BuildContext;
use crate::build::job_variables::JobVariables;
use crate::build::package::Package;
use crate::build::package::template::{Template, Var};
use crate::config::Build;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

impl BuildContext {
    pub fn setup_rpm(&self) -> Package {
        let specfile_path_template_path = self.config.rpm.clone().unwrap().spec_template;

        let rpmbuild_folder_name = format!("{}-{}", self.config.package_name, self.distro.id);
        let rpmbuild_path = self.build_dir.join(&rpmbuild_folder_name);
        std::fs::create_dir_all(&rpmbuild_path).unwrap_or_else(|e| panic!("cannot create directory {}: {}", rpmbuild_path.display(), e));

        let source_folder_name = format!("{}-{}", self.config.package_name, self.job_variables.version);
        let specfile_name = format!("{}-{}.spec", source_folder_name, self.distro.id);

        let mut vars: HashMap<String, Var> = self.job_variables.to_vars();
        vars.extend(self.config.to_vars());
        vars.insert("source_folder_name".to_string(), source_folder_name.into());
        let template = Template::new(self.source_path.join(&specfile_path_template_path));
        template.render_to_file(vars, self.build_dir.join(&rpmbuild_folder_name).join(&specfile_name));

        Package {
            mounts: HashMap::new(),
            commands: Vec::new(),
            source_path: self.source_path.clone(),
            output_path: rpmbuild_path.clone(),
        }
    }
}
