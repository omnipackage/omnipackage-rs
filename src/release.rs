use crate::artefacts;
use crate::build::{BuildContext, extract_version, job_variables};
use crate::config::{Build, Config, Repository};
use crate::distros::{Distro, Distros};
use crate::publish::PublishContext;
use crate::{JobArgs, LoggingArgs, ProjectArgs};
use std::error::Error;
use std::path::PathBuf;

struct JobSetup {
    job_variables: job_variables::JobVariables,
    build_dir: PathBuf,
    source_dir: PathBuf,
}

impl JobSetup {
    fn new(project: &ProjectArgs, job: &JobArgs, config: &Config) -> Result<Self, Box<dyn Error>> {
        let version = extract_version::extract_version(&project.source_dir, &config.extract_version);
        let job_variables = job_variables::JobVariables::build(version).with_secrets(config.secrets.clone().into_iter().collect());

        Ok(Self {
            job_variables,
            build_dir: PathBuf::from(&job.build_dir),
            source_dir: project.source_dir.clone(),
        })
    }

    fn build_context(&self, distro: &'static Distro, build_config: &Build, logging: &LoggingArgs) -> BuildContext {
        BuildContext {
            distro,
            source_dir: self.source_dir.clone(),
            config: build_config.clone(),
            job_variables: self.job_variables.clone(),
            build_dir: self.build_dir.clone(),
            logging_args: logging.clone(),
        }
    }

    fn publish_context(&self, distro: &'static Distro, build_config: &Build, repository_config: &Repository, logging: &LoggingArgs) -> PublishContext {
        let distro_build_dir = self.build_dir.join(build_config.build_folder_name());
        let artefacts = artefacts::find_artefacts_in_build_dir(distro, &distro_build_dir);
        PublishContext {
            distro,
            logging_args: logging.clone(),
            config: repository_config.clone(),
            artefacts,
            build_dir: distro_build_dir,
        }
    }
}

pub fn build(project: ProjectArgs, job: JobArgs, logging: LoggingArgs) -> Result<(), Box<dyn Error>> {
    let config = project.load_config(false)?;
    let setup = JobSetup::new(&project, &job, &config)?;
    let mut any_failed = false;

    for build_config in detect_builds(job.clone(), config) {
        let distro = Distros::get().by_id(&build_config.distro);
        let build_ok = fail_fast_or_continue(setup.build_context(distro, &build_config, &logging).run(), job.fail_fast)?;

        if !build_ok {
            any_failed = true;
        }
    }

    if any_failed { Err("build one or more distros failed".into()) } else { Ok(()) }
}

pub fn publish(project: ProjectArgs, job: JobArgs, logging: LoggingArgs, repository: Option<String>) -> Result<(), Box<dyn Error>> {
    let config = project.load_config(false)?;
    let setup = JobSetup::new(&project, &job, &config)?;
    let repository_config = config.repositories.find_by_name_or_default(repository.as_deref())?.clone();
    let mut any_failed = false;

    for build_config in detect_builds(job.clone(), config) {
        let distro = Distros::get().by_id(&build_config.distro);
        let publish_ok = fail_fast_or_continue(setup.publish_context(distro, &build_config, &repository_config, &logging).run(), job.fail_fast)?;

        if !publish_ok {
            any_failed = true;
        }
    }

    if any_failed { Err("publish one or more distros failed".into()) } else { Ok(()) }
}

pub fn release(project: ProjectArgs, job: JobArgs, logging: LoggingArgs, repository: Option<String>) -> Result<(), Box<dyn Error>> {
    let config = project.load_config(false)?;
    let setup = JobSetup::new(&project, &job, &config)?;
    let repository_config = config.repositories.find_by_name_or_default(repository.as_deref())?.clone();
    let mut any_failed = false;

    for build_config in detect_builds(job.clone(), config) {
        let distro = Distros::get().by_id(&build_config.distro);
        let build_ok = fail_fast_or_continue(setup.build_context(distro, &build_config, &logging).run(), job.fail_fast)?;

        if build_ok {
            let publish_ok = fail_fast_or_continue(setup.publish_context(distro, &build_config, &repository_config, &logging).run(), job.fail_fast)?;
            any_failed |= !publish_ok;
        } else {
            any_failed = true;
        }
    }

    if any_failed { Err("release one or more distros failed".into()) } else { Ok(()) }
}

fn fail_fast_or_continue(result: Result<(), Box<dyn Error>>, fail_fast: bool) -> Result<bool, Box<dyn Error>> {
    match result {
        Ok(()) => Ok(true),
        Err(e) if fail_fast => Err(e),
        Err(_) => Ok(false),
    }
}

fn detect_builds(job: JobArgs, config: Config) -> impl Iterator<Item = Build> {
    config
        .builds
        .into_iter()
        .filter(move |build| Distros::get().contains(&build.distro))
        .filter(move |build| job.distros.is_empty() || job.distros.contains(&build.distro))
}
