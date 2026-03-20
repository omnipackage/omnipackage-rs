use crate::PublishArgs;
use crate::config::{Config, Repository};
use crate::distros::{Distro, Distros};
use crate::logger::{Color, Logger, colorize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PublishContext {
    pub distro: &'static Distro,
    args: PublishArgs,
    config: Repository,
    artefacts: Vec<PathBuf>,
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
            })
        })
        .collect();

    Logger::new().info(format!(
        "found artefacts for {}",
        contexts.iter().map(|c| colorize(Color::BoldCyan, &c.distro.name)).collect::<Vec<_>>().join(", ")
    ));

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
    pub fn run(&self) {}
}
