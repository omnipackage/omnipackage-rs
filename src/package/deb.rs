use crate::build::job_variables::JobVariables;
use crate::config::{Build, Repository};
use crate::distros::Distro;
use crate::gpg::{Gpg, Key};
use crate::package::Package;
use crate::template::{Template, Var};
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Deb {
    pub distro: &'static Distro,
    pub source_dir: PathBuf,
    pub job_variables: JobVariables,
    pub distro_build_dir: PathBuf,

    mounts: HashMap<String, String>,
    commands: Vec<String>,
    build_output_dir: PathBuf,
    setup_stages: Vec<String>,
    gpgkey: Option<Key>,
}

impl Deb {
    pub fn new(distro: &'static Distro, source_dir: PathBuf, job_variables: JobVariables, distro_build_dir: PathBuf) -> Self {
        Self {
            distro,
            source_dir,
            job_variables,
            distro_build_dir: distro_build_dir.clone(),
            mounts: HashMap::new(),
            commands: Vec::new(),
            build_output_dir: distro_build_dir.clone(),
            setup_stages: Vec::new(),
            gpgkey: None,
        }
    }
}

impl Package for Deb {
    fn clone_box(&self) -> Box<dyn Package> {
        Box::new(self.clone())
    }

    fn source_dir(&self) -> PathBuf {
        self.source_dir.clone()
    }

    fn distro_build_dir(&self) -> PathBuf {
        self.distro_build_dir.clone()
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

    fn build_output_dir(&self) -> PathBuf {
        self.build_output_dir.clone()
    }

    fn setup_stages(&self) -> Vec<String> {
        self.setup_stages.clone()
    }

    fn gpgkey(&self) -> Option<Key> {
        self.gpgkey.clone()
    }

    fn setup_build(&mut self, config: Build) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn setup_repository(&mut self, config: Repository) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}
