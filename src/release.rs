use crate::config::{Build, Config};
use crate::distros::Distros;
use crate::package::{Package, make_package};
use crate::publish::Publish;
use crate::runner::Runner;
use crate::{BuildArgs, JobArgs, ProjectArgs, PublishArgs, ReleaseArgs};
use crate::{extract_version, job_variables};
use anyhow::Result;
use std::path::PathBuf;

struct JobSetup {
    job_variables: job_variables::JobVariables,
    build_dir: PathBuf,
    source_dir: PathBuf,
}

impl JobSetup {
    fn new(project: &ProjectArgs, job: &JobArgs, config: &Config, version_extractor: &Option<String>) -> Result<Self, anyhow::Error> {
        let version_config = config.version_extractors.find_by_name_or_default(version_extractor.as_deref())?.clone();
        let version = extract_version::extract_version(&project.source_dir, &version_config)?;
        let job_variables = job_variables::JobVariables::new(version).with_secrets(config.secrets.clone().into_iter().collect());

        Ok(Self {
            job_variables,
            build_dir: job.build_dir.clone(),
            source_dir: project.source_dir.clone(),
        })
    }

    pub fn make_package(&self, distro_id: &str, package_name: &str) -> Result<Box<dyn Package>, anyhow::Error> {
        let distro = Distros::get().by_id(distro_id);

        make_package(
            distro.clone(),
            self.source_dir.clone(),
            self.job_variables.clone(),
            self.build_dir.join(format!("{}-{}", package_name, distro_id)),
        )
    }
}

pub fn build(args: BuildArgs) -> Result<(), anyhow::Error> {
    let config = args.project.load_config(false)?;
    let setup = JobSetup::new(&args.project, &args.job, &config, &args.version_extractor)?;
    let mut any_failed = false;

    for build_config in detect_builds(args.job.clone(), config) {
        let mut pkg = setup.make_package(&build_config.distro, &build_config.package_name)?;
        pkg.setup_build(build_config.clone())?;

        let builder = Runner::new(pkg.clone(), args.logging.clone(), setup.job_variables.clone());
        let build_ok = fail_fast_or_continue(builder.run(), args.job.fail_fast)?;

        if !build_ok {
            any_failed = true;
        }
    }

    if any_failed { Err(anyhow::anyhow!("build one or more distros failed")) } else { Ok(()) }
}

pub fn publish(args: PublishArgs) -> Result<(), anyhow::Error> {
    let config = args.project.load_config(false)?;
    let setup = JobSetup::new(&args.project, &args.job, &config, &None)?;
    let repository_config = config.repositories.find_by_name_or_default(args.repository.as_deref())?.clone();
    let mut any_failed = false;

    for build_config in detect_builds(args.job.clone(), config) {
        let mut pkg = setup.make_package(&build_config.distro, &build_config.package_name)?;
        pkg.setup_repository(repository_config.clone())?;

        let runner = Runner::new(pkg.clone(), args.logging.clone(), setup.job_variables.clone());
        let build_ok = fail_fast_or_continue(runner.run(), args.job.fail_fast)?;
        if build_ok {
            let publisher = Publish::new(pkg.clone(), repository_config.clone(), args.custom_install_page.clone());
            let publish_ok = fail_fast_or_continue(publisher.run(), args.job.fail_fast)?;
            any_failed |= !publish_ok;
        } else {
            any_failed = true;
        }
    }

    if any_failed { Err(anyhow::anyhow!("publish one or more distros failed")) } else { Ok(()) }
}

pub fn release(args: ReleaseArgs) -> Result<(), anyhow::Error> {
    let config = args.project.load_config(false)?;
    let setup = JobSetup::new(&args.project, &args.job, &config, &args.version_extractor)?;
    let repository_config = config.repositories.find_by_name_or_default(args.repository.as_deref())?.clone();
    let mut any_failed = false;

    for build_config in detect_builds(args.job.clone(), config) {
        let mut pkg = setup.make_package(&build_config.distro, &build_config.package_name)?;
        pkg.setup_build(build_config.clone())?;
        pkg.setup_repository(repository_config.clone())?;

        let runner = Runner::new(pkg.clone(), args.logging.clone(), setup.job_variables.clone());
        let build_ok = fail_fast_or_continue(runner.run(), args.job.fail_fast)?;
        if build_ok {
            let publisher = Publish::new(pkg.clone(), repository_config.clone(), args.custom_install_page.clone());
            let publish_ok = fail_fast_or_continue(publisher.run(), args.job.fail_fast)?;
            any_failed |= !publish_ok;
        } else {
            any_failed = true;
        }
    }

    if any_failed { Err(anyhow::anyhow!("release one or more distros failed")) } else { Ok(()) }
}

pub fn fail_fast_or_continue(result: Result<(), anyhow::Error>, fail_fast: bool) -> Result<bool, anyhow::Error> {
    match result {
        Ok(()) => Ok(true),
        Err(e) if fail_fast => Err(e),
        Err(_) => Ok(false),
    }
}

pub fn detect_builds(job: JobArgs, config: Config) -> impl Iterator<Item = Build> {
    config
        .builds
        .into_iter()
        .filter(move |build| Distros::get().contains(&build.distro))
        .filter(move |build| job.distros.is_empty() || job.distros.contains(&build.distro))
}
