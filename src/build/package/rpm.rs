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
        vars.insert("source_folder_name".to_string(), source_folder_name.clone().into());
        let template = Template::new(self.source_path.join(&specfile_path_template_path));
        template.render_to_file(vars, self.build_dir.join(&rpmbuild_folder_name).join(&specfile_name));

        let mut mounts: HashMap<String, String> = HashMap::new();
        mounts.insert(self.source_path.to_string_lossy().to_string(), "/source".to_string());
        mounts.insert(rpmbuild_path.to_string_lossy().to_string(), "/root/rpmbuild".to_string());

        let mut commands: Vec<String> = self.distro.setup(&self.config.build_dependencies);
        if let Some(bbs) = self.before_build_script("/source") {
            commands.push(bbs);
        }
        commands.extend([
            "rpmdev-setuptree".to_string(),
            "rm -rf /root/rpmbuild/SOURCES/*".to_string(),
            format!("cp -R /source /root/rpmbuild/SOURCES/{source_folder_name}"),
            format!("tar -cvzf {source_folder_name}.tar.gz {source_folder_name}/"),
            format!("cd /root/rpmbuild/SOURCES/{source_folder_name}/"),
            format!("QA_RPATHS=$(( 0x0001|0x0010|0x0002|0x0004|0x0008|0x0020 )) rpmbuild --clean -bb /root/rpmbuild/{specfile_name}"),
        ]);

        Package {
            mounts,
            commands,
            source_path: self.source_path.clone(),
            output_path: rpmbuild_path.clone(),
        }
    }
}
