use crate::PublishArgs;
use crate::config::{Config, Repository};
use crate::distros::{Distro, Distros};
use crate::logger::{Color, Logger, colorize};
use std::path::{Path, PathBuf};

mod deb;
mod rpm;
mod s3;

#[derive(Debug, Clone)]
pub struct PublishContext {
    pub distro: &'static Distro,
    args: PublishArgs,
    config: Repository,
    artefacts: Vec<PathBuf>,
    build_dir: PathBuf,
}

pub fn run(args: &PublishArgs) -> Result<(), String> {
    let config = Config::load_with_env(&args.project.source_dir.join(&args.project.config_path), &args.project.env_file)?;

    let repository_config = config.repositories.find_by_name_or_default(args.repository.as_deref())?;

    let contexts: Vec<PublishContext> = config
        .builds
        .iter()
        .filter(|build| Distros::get().contains(&build.distro))
        .filter(|build| args.distros.is_empty() || args.distros.contains(&build.distro))
        .filter_map(|build| {
            let build_dir = PathBuf::from(&args.build_dir).join(build.build_folder_name());
            if !build_dir.exists() {
                return None;
            }

            let distro = Distros::get().by_id(&build.distro);
            let artefacts = find_artefacts(distro, &build_dir);
            if artefacts.is_empty() {
                return None;
            }

            Some(PublishContext {
                distro,
                args: args.clone(),
                config: repository_config.clone(),
                artefacts,
                build_dir,
            })
        })
        .collect();

    Logger::new().info(format!(
        "found artefacts for {}",
        contexts.iter().map(|c| colorize(Color::BoldCyan, &c.distro.name)).collect::<Vec<_>>().join(", ")
    ));

    contexts.iter().for_each(|c| {
        c.run();
    });

    Ok(())
}

fn find_artefacts(distro: &'static Distro, build_dir: &Path) -> Vec<PathBuf> {
    let pattern = match distro.package_type.as_str() {
        "rpm" => build_dir.join("RPMS/**/*.rpm"),
        "deb" => build_dir.join("output").join("*.deb"), // NOTE: copy-paste, same logic happens in Package build
        _ => panic!("unknown package type {}", distro.package_type),
    };

    glob::glob(pattern.to_str().unwrap()).unwrap().filter_map(|e| e.ok()).collect()
}

impl PublishContext {
    pub fn run(&self) {
        Logger::new().info(format!("starting repository publish for {}", self.distro.id));

        let result = self.within_repository_dir(|dir| {
            match self.config.provider.as_str() {
                "s3" => {
                    let s3_config = self.config.s3();
                    let s3 = s3::S3::new(s3_config, format!("/{}", self.distro.id));
                    if !s3.bucket_exists()? {
                        return Err(format!("bucket '{}' does not exist", s3_config.bucket));
                    }
                    s3.download_all(dir)?;
                    self.setup_repo(dir)?;
                    s3.upload_all(dir)?;
                    s3.delete_deleted_files(dir)?;
                }
                &_ => todo!(),
            }

            Ok(())
        });

        match result {
            Ok(_) => Logger::new().info(format!("done repository publish for {}", self.distro.id)),
            Err(msg) => Logger::new().error(format!("error repository publish for {}: {}", self.distro.id, msg)),
        };
    }

    fn within_repository_dir<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Path) -> Result<R, String>,
    {
        let dir = self.build_dir.join("repository");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("cannot clear repository dir {}: {}", dir.display(), e))?;
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create repository dir {}: {}", dir.display(), e))?;
        f(&dir)
    }

    fn setup_repo(&self, dir: &Path) -> Result<(), String> {
        for artefact in &self.artefacts {
            let dest = dir.join(artefact.file_name().unwrap_or_else(|| artefact.as_os_str()));
            std::fs::copy(artefact, &dest).map_err(|e| format!("cannot copy {} to {}: {}", artefact.display(), dest.display(), e))?;
        }

        match self.distro.package_type.as_str() {
            "rpm" => self.setup_rpm_repo(dir),
            "deb" => self.setup_deb_repo(dir),
            _ => Err(format!("unknown package type {}", self.distro.package_type)),
        }
    }
}
