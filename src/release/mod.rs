use crate::build::{BuildContext, extract_version, job_variables};
use crate::config::{Build, Config};
use crate::distros::{Distro, Distros};
use crate::logger::{LogOutput, Logger};
use crate::publish::PublishContext;
use crate::{BuildArgs, JobArgs, LoggingArgs, ProjectArgs, ReleaseArgs};
use std::path::PathBuf;

pub fn detect_builds(job: JobArgs, config: Config) -> impl Iterator<Item = Build> {
    config
        .builds
        .into_iter()
        .filter(move |build| Distros::get().contains(&build.distro))
        .filter(move |build| job.distros.is_empty() || job.distros.contains(&build.distro))
}

pub fn run(args: ReleaseArgs) -> Result<(), Box<dyn std::error::Error>> {
    let config = args.project.load_config()?;

    let version = extract_version::extract_version(&args.project.source_dir, &config.extract_version);
    let job_variables = job_variables::JobVariables::build(version.clone()).with_secrets(config.secrets.clone().into_iter().collect());

    let repositories = config.repositories.clone();
    let repository_config = repositories.find_by_name_or_default(args.repository.as_deref())?;

    let build_dir = PathBuf::from(&args.job.build_dir);
    let logging = args.logging.clone();
    let source_dir = args.project.source_dir.clone();

    detect_builds(args.job, config).for_each(|build_config| {
        let distro = Distros::get().by_id(&build_config.distro);
        let build_result = BuildContext {
            distro,
            source_dir: source_dir.clone(),
            config: build_config.clone(),
            job_variables: job_variables.clone(),
            build_dir: build_dir.clone(),
            logging_args: logging.clone(),
        }
        .run();

        if build_result.success {
            let publish_result = PublishContext {
                distro,
                logging_args: logging.clone(),
                config: repository_config.clone(),
                artefacts: build_result.artefacts,
                build_dir: build_result.distro_build_dir,
            }
            .run();
        }
    });

    Ok(())
}
