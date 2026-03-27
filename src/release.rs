use crate::build::{BuildContext, extract_version, job_variables};
use crate::config::{Build, Config};
use crate::distros::{Distro, Distros};
use crate::logger::{Color, LogOutput, Logger, colorize};
use crate::publish::PublishContext;
use crate::publish::artefacts;
use crate::{BuildArgs, JobArgs, LoggingArgs, ProjectArgs, PublishArgs, ReleaseArgs};
use std::error::Error;
use std::path::PathBuf;

pub fn build(project: ProjectArgs, job: JobArgs, logging: LoggingArgs) -> Result<(), Box<dyn Error>> {
    Ok(())
}

pub fn publish(project: ProjectArgs, job: JobArgs, logging: LoggingArgs, repository: Option<String>) -> Result<(), Box<dyn Error>> {
    Ok(())
}

pub fn release(project: ProjectArgs, job: JobArgs, logging: LoggingArgs, repository: Option<String>) -> Result<(), Box<dyn Error>> {
    let config = project.load_config()?;

    let version = extract_version::extract_version(&project.source_dir, &config.extract_version);
    let job_variables = job_variables::JobVariables::build(version.clone()).with_secrets(config.secrets.clone().into_iter().collect());

    let repositories = config.repositories.clone();
    let repository_config = repositories.find_by_name_or_default(repository.as_deref())?;

    let build_dir = PathBuf::from(&job.build_dir);
    let logging = logging.clone();
    let source_dir = project.source_dir.clone();

    for build_config in detect_builds(job.clone(), config) {
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

        if build_result.is_err() && job.fail_fast {
            return build_result;
        }

        if build_result.is_ok() {
            let distro_build_dir = PathBuf::from(&job.build_dir).join(build_config.build_folder_name());
            if !distro_build_dir.exists() {
                return Err(format!("distro build dir '{}' does not exist", distro_build_dir.display()).into());
            }
            let artefacts = artefacts::find_artefacts_in_build_dir(distro, &distro_build_dir);
            if artefacts.is_empty() {
                return Err(format!("no artefacts in {}", distro_build_dir.display()).into());
            }

            let publish_result = PublishContext {
                distro,
                logging_args: logging.clone(),
                config: repository_config.clone(),
                artefacts,
                build_dir: distro_build_dir,
            }
            .run();
            if publish_result.is_err() && job.fail_fast {
                return publish_result;
            }
        }
    }

    Ok(())
}

fn detect_builds(job: JobArgs, config: Config) -> impl Iterator<Item = Build> {
    config
        .builds
        .into_iter()
        .filter(move |build| Distros::get().contains(&build.distro))
        .filter(move |build| job.distros.is_empty() || job.distros.contains(&build.distro))
}
