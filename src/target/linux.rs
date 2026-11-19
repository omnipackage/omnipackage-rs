use super::{Target, TargetContext};
use crate::config::{Build, Repository};
use crate::logger::Logger;
use crate::package::Package;
use crate::publish::{Publish, prepopulate_with_retention};
use crate::release::fail_fast_or_continue;
use crate::runner::Runner;
use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;

pub struct LinuxTarget {
    build: Build,
    pkg: Box<dyn Package>,
}

impl LinuxTarget {
    pub fn new(ctx: &TargetContext, build: Build) -> Result<Self> {
        let pkg = ctx.job.make_package(&build.distro, &build.package_name)?;
        Ok(Self { build, pkg })
    }

    fn run_and_publish(&self, ctx: &TargetContext, repository: &Repository, custom_install_page: Option<PathBuf>) -> Result<bool> {
        let skip_upload = apply_retention(repository, self.pkg.as_ref())?;

        let runner = Runner::new(self.pkg.clone(), ctx.logging.clone(), ctx.job.job_variables.clone());
        let build_ok = fail_fast_or_continue(runner.run(), ctx.fail_fast)?;
        if !build_ok {
            return Ok(false);
        }
        let publisher = Publish::new(self.pkg.clone(), repository.clone(), custom_install_page, skip_upload);
        fail_fast_or_continue(publisher.run(), ctx.fail_fast)
    }
}

impl Target for LinuxTarget {
    fn build(&mut self, ctx: &TargetContext) -> Result<bool> {
        self.pkg.setup_build(self.build.clone())?;

        let builder = Runner::new(self.pkg.clone(), ctx.logging.clone(), ctx.job.job_variables.clone());
        fail_fast_or_continue(builder.run(), ctx.fail_fast)
    }

    fn publish(&mut self, ctx: &TargetContext, repository: &Repository, custom_install_page: Option<PathBuf>) -> Result<bool> {
        self.pkg.setup_repository(repository.clone())?;
        self.run_and_publish(ctx, repository, custom_install_page)
    }

    fn release(&mut self, ctx: &TargetContext, repository: &Repository, custom_install_page: Option<PathBuf>) -> Result<bool> {
        self.pkg.setup_build(self.build.clone())?;
        self.pkg.setup_repository(repository.clone())?;
        self.run_and_publish(ctx, repository, custom_install_page)
    }
}

fn apply_retention(repository_config: &Repository, pkg: &dyn Package) -> Result<HashSet<PathBuf>, anyhow::Error> {
    match prepopulate_with_retention(repository_config, pkg)? {
        Some(stats) => {
            Logger::new().info(format!("retention for {}: kept {}, removed {}", pkg.distro().id, stats.kept, stats.removed));
            Ok(stats.retained_files.into_iter().collect())
        }
        None => Ok(HashSet::new()),
    }
}
