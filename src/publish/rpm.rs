use crate::gpg::{Gpg, Key};
use crate::publish::PublishContext;
use std::error::Error;
use std::path::{Path, PathBuf};

impl PublishContext {
    pub fn setup_rpm_repo(&self, key: &Key, home_dir: &Path, work_dir: &Path) -> Result<(), Box<dyn Error>> {
        let key_id = Gpg::new().key_id(&key.priv_key)?;
        self.write_rpmmacros(home_dir, &key_id)?;

        let mut commands = self.import_gpg_keys_commands();
        commands.extend([
            "rpm --import public.key".to_string(),
            "rpm --addsign *.rpm".to_string(),
            "createrepo --retain-old-md=0 --compatibility .".to_string(),
            "gpg --no-tty --batch --detach-sign --armor --verbose --yes --always-trust repodata/repomd.xml".to_string(),
            "mv public.key repodata/repomd.xml.key".to_string(),
        ]);

        self.execute(commands, home_dir, work_dir)?;
        self.write_repo_file(work_dir, &self.config.project_slug(), &self.distro.name, &self.distro_url())
    }

    fn write_repo_file(&self, work_dir: &Path, project_slug: &str, distro_name: &str, distro_url: &str) -> Result<(), Box<dyn Error>> {
        let content = format!(
            "[{project_slug}]\n\
             name={project_slug} ({distro_name})\n\
             type=rpm-md\n\
             baseurl={distro_url}\n\
             gpgcheck=1\n\
             gpgkey={distro_url}/repodata/repomd.xml.key\n\
             enabled=1\n"
        );

        Ok(std::fs::write(work_dir.join(format!("{}.repo", project_slug)), content)?)
    }

    fn write_rpmmacros(&self, home_dir: &Path, gpg_key_id: &str) -> Result<(), Box<dyn Error>> {
        let content = format!(
            "%_signature gpg\n\
             %_gpg_name {gpg_key_id}\n"
        );

        Ok(std::fs::write(home_dir.join(".rpmmacros"), content)?)
    }
}
