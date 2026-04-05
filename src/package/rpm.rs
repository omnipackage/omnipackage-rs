use crate::build::job_variables::JobVariables;
use crate::config::{Build, Repository};
use crate::distros::Distro;
use crate::package::Package;
use std::collections::HashMap;
use crate::template::{Template, Var};
use std::path::{Path, PathBuf};
use std::error::Error;
use crate::gpg::{Key, Gpg};

#[derive(Debug, Clone)]
pub struct Rpm {
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

impl Rpm {
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

    fn write_repo_file(&self, repo_dir: &Path, project_slug: &str, distro_name: &str, distro_url: &str) -> Result<(), Box<dyn Error>> {
        let content = format!(
            "[{project_slug}]\n\
             name={project_slug} ({distro_name})\n\
             type=rpm-md\n\
             baseurl={distro_url}\n\
             gpgcheck=1\n\
             gpgkey={distro_url}/repodata/repomd.xml.key\n\
             enabled=1\n"
        );

        Ok(std::fs::write(repo_dir.join(format!("{}.repo", project_slug)), content)?)
    }

    fn write_rpmmacros(&self, home_dir: &Path, gpg_key_id: &str) -> Result<(), Box<dyn Error>> {
        let content = format!(
            "%_signature gpg\n\
             %_gpg_name {gpg_key_id}\n"
        );

        Ok(std::fs::write(home_dir.join(".rpmmacros"), content)?)
    }
}

impl Package for Rpm {
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

    fn setup_build(&mut self, config: Build) -> Result<(), Box<dyn Error>> {
        let specfile_path_template_path = config.rpm.clone().ok_or("rpm config is missing")?.spec_template;

        let rpmbuild_path = self.distro_build_dir();
        std::fs::create_dir_all(&rpmbuild_path).map_err(|e| format!("cannot create directory {}: {}", rpmbuild_path.display(), e))?;

        let source_folder_name = format!("{}-{}", config.package_name, self.job_variables.version);
        let specfile_name = format!("{}-{}.spec", source_folder_name, self.distro.id);

        let mut template_vars: HashMap<String, Var> = self.job_variables.to_template_vars();
        template_vars.extend(config.to_template_vars());
        template_vars.insert("source_folder_name".to_string(), source_folder_name.clone().into());
        let template = Template::from_file(self.source_dir.join(&specfile_path_template_path))?;
        template.render_to_file(template_vars, rpmbuild_path.join(&specfile_name))?;

        self.mounts.insert(self.source_dir.to_string_lossy().to_string(), "/source".to_string());
        self.mounts.insert(rpmbuild_path.to_string_lossy().to_string(), "/root/rpmbuild".to_string());

        self.commands.extend(self.distro.setup(&config.build_dependencies));
        if let Some(bbs) = self.before_build_script("/source", &config) {
            self.commands.push(bbs);
        }
        self.commands.extend([
            "rpmdev-setuptree".to_string(),
            "rm -rf /root/rpmbuild/SOURCES/*".to_string(),
            format!("cp -R /source /root/rpmbuild/SOURCES/{source_folder_name}"),
            "cd /root/rpmbuild/SOURCES/".to_string(),
            format!("tar -cvzf {source_folder_name}.tar.gz --exclude='.git' --exclude='node_modules' {source_folder_name}/"),
            format!("cd /root/rpmbuild/SOURCES/{source_folder_name}/"),
            format!("QA_RPATHS=$(( 0x0001|0x0010|0x0002|0x0004|0x0008|0x0020 )) rpmbuild --clean -bb /root/rpmbuild/{specfile_name}"),
        ]);

        self.build_output_dir = rpmbuild_path.join("RPMS");
        self.setup_stages.push("build".to_string());

        Ok(())
    }

    fn setup_repository(&mut self, config: Repository) -> Result<(), Box<dyn Error>> {
        let gpgkey = self.prepare_gpgkey(&config)?;
        let (home_dir, repo_dir) = self.prepare_repository(&gpgkey)?;

        let key_id = Gpg::new().key_id(&gpgkey.priv_key)?;
        self.write_rpmmacros(&home_dir, &key_id)?;

        self.mounts.insert(home_dir.to_string_lossy().to_string(), "/root".to_string());
        self.mounts.insert(repo_dir.to_string_lossy().to_string(), "/repo".to_string());

        self.commands.extend(self.distro.setup_repo.clone());
        self.commands.extend(self.import_gpg_keys_commands());
        self.commands.extend([
            "cd /repo".to_string(),
            "cp /root/rpmbuild/RPMS/**/*.rpm /repo/".to_string(),
            "rpm --import public.key".to_string(),
            "rpm --addsign *.rpm".to_string(),
            "createrepo --retain-old-md=0 --compatibility .".to_string(),
            "gpg --no-tty --batch --detach-sign --armor --verbose --yes --always-trust repodata/repomd.xml".to_string(),
            "mv public.key repodata/repomd.xml.key".to_string(),
        ]);

        self.build_output_dir = repo_dir.clone();
        self.setup_stages.push("repository".to_string());
        self.gpgkey = Some(gpgkey);

        self.write_repo_file(&repo_dir, &config.project_slug(), &self.distro.name, &self.distro_url(&config))
    }
}
