use crate::build::job_variables::JobVariables;
use crate::build::package::Package;
use crate::build::package::template::{Template, Var};
use crate::config::Build;
use crate::distros::Distro;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct Rpm {
    pub build_config: Build,
    pub build_dir: PathBuf,
    pub job_variables: JobVariables,
    pub source_path: PathBuf,
    pub distro: &'static Distro,
}

impl Package for Rpm {
    fn setup(&self) {
        let specfile_path_template_path = self.build_config.rpm.clone().unwrap().spec_template;

        let rpmbuild_folder_name = format!("{}-{}", self.build_config.package_name, self.distro.name);

        let rpmbuild_path = self.build_dir.join(&rpmbuild_folder_name);
        std::fs::create_dir_all(&rpmbuild_path).unwrap_or_else(|e| panic!("cannot create directory {}: {}", rpmbuild_path.display(), e));
        // @output_path = rpmbuild_path

        let source_folder_name = format!("{}-{}", self.build_config.package_name, self.job_variables.version);
        let specfile_name = format!("{}-{}.spec", source_folder_name, self.distro.name);

        let mut vars: HashMap<String, Var> = self.job_variables.to_vars();
        vars.extend(self.build_config.to_vars());
        vars.insert("source_folder_name".to_string(), source_folder_name.into());
        let template = Template::new(self.source_path.join(&specfile_path_template_path));
        let result = template.render(vars);
        println!("{}", result);
    }

    fn output_path(&self) -> PathBuf {
        "123".into()
    }

    fn mounts(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn commands(&self) -> Vec<String> {
        Vec::new()
    }
}
