use crate::build::job_variables::JobVariables;
use crate::config::{Build, Repository};
use crate::distros::Distro;
use crate::package::Package;
use std::collections::HashMap;
use crate::template::{Template, Var};
use std::path::{Path, PathBuf};
use std::error::Error;
use crate::gpg::{Key, Gpg};

pub struct Rpm {
    pub distro: &'static Distro,
    pub source_dir: PathBuf,
    pub build_config: Build,
    pub repository_config: Repository,
    pub job_variables: JobVariables,
    pub build_dir: PathBuf,

    mounts: HashMap<String, String>,
    commands: Vec<String>,
}

impl Rpm {
    pub fn new(distro: &'static Distro, source_dir: PathBuf, build_config: Build, repository_config: Repository, job_variables: JobVariables, build_dir: PathBuf) -> Self {
        Self {
            distro,
            source_dir,
            build_config,
            repository_config,
            job_variables,
            build_dir,
            mounts: HashMap::new(),
            commands: Vec::new(),
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

    fn build_config(&self) -> Build {
        self.build_config.clone()
    }

    fn repository_config(&self) -> Repository {
        self.repository_config.clone()
    }

    fn build_dir(&self) -> PathBuf {
        self.build_dir.clone()
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

    fn build(&mut self) -> Result<(), Box<dyn Error>> {
        let specfile_path_template_path = self.build_config.rpm.clone().ok_or("rpm config is missing")?.spec_template;

        let rpmbuild_path = self.distro_build_dir();
        std::fs::create_dir_all(&rpmbuild_path).map_err(|e| format!("cannot create directory {}: {}", rpmbuild_path.display(), e))?;

        let source_folder_name = format!("{}-{}", self.build_config.package_name, self.job_variables.version);
        let specfile_name = format!("{}-{}.spec", source_folder_name, self.distro.id);

        let mut template_vars: HashMap<String, Var> = self.job_variables.to_template_vars();
        template_vars.extend(self.build_config.to_template_vars());
        template_vars.insert("source_folder_name".to_string(), source_folder_name.clone().into());
        let template = Template::from_file(self.source_dir.join(&specfile_path_template_path))?;
        template.render_to_file(template_vars, rpmbuild_path.join(&specfile_name))?;

        self.mounts.insert(self.source_dir.to_string_lossy().to_string(), "/source".to_string());
        self.mounts.insert(rpmbuild_path.to_string_lossy().to_string(), "/root/rpmbuild".to_string());

        self.commands.extend(self.distro.setup(&self.build_config.build_dependencies));
        self.commands.extend(self.distro.setup_repo.clone());
        if let Some(bbs) = self.before_build_script("/source") {
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

        Ok(())
    }

    fn publish(&mut self) -> Result<(), Box<dyn Error>> {
        self.publish_prepare()?;
        let key = self.repository_config().gpg_private_key()?;
        let key_id = Gpg::new().key_id(&key)?;
        self.write_rpmmacros(&self.home_dir(), &key_id)?;

        self.mounts.extend(self.publish_mounts());
        self.commands.extend(self.import_gpg_keys_commands());
        self.commands.extend([
            "rpm --import public.key".to_string(),
            "rpm --addsign *.rpm".to_string(),
            "cp /root/rpmbuild/RPMS/**/*.rpm /repo/".to_string(),
            "createrepo --retain-old-md=0 --compatibility .".to_string(),
            "gpg --no-tty --batch --detach-sign --armor --verbose --yes --always-trust repodata/repomd.xml".to_string(),
            "mv public.key repodata/repomd.xml.key".to_string(),
        ]);

        self.write_repo_file(&self.repo_dir(), &self.repository_config.project_slug(), &self.distro.name, &self.distro_url())
    }
}
