use crate::gpg::{Gpg, Key};
use crate::publish::PublishContext;
use std::path::{Path, PathBuf};

impl PublishContext {
    pub fn setup_rpm_repo(&self, key: &Key, home_dir: &Path, work_dir: &Path) -> Result<(), String> {
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
        self.write_repo_file(work_dir, &self.config.package_name, &self.distro.name, &self.distro_url())
    }

    fn write_repo_file(&self, work_dir: &Path, project_slug: &str, distro_name: &str, distro_url: &str) -> Result<(), String> {
        let content = format!(
            "[{project_slug}]\n\
             name={project_slug} ({distro_name})\n\
             type=rpm-md\n\
             baseurl={distro_url}\n\
             gpgcheck=1\n\
             gpgkey={distro_url}/repodata/repomd.xml.key\n\
             enabled=1\n"
        );

        std::fs::write(work_dir.join(format!("{}.repo", project_slug)), content).map_err(|e| format!("cannot write repo file: {}", e))
    }

    fn write_rpmmacros(&self, home_dir: &Path, gpg_key_id: &str) -> Result<(), String> {
        let content = format!(
            "%_signature gpg\n\
             %_gpg_name {gpg_key_id}\n"
        );

        std::fs::write(home_dir.join(".rpmmacros"), content).map_err(|e| format!("cannot write rpmmacros: {}", e))
    }
}
