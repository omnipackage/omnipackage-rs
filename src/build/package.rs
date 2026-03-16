use crate::build::job_variables::JobVariables;

pub trait Package {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn source_path(&self) -> &str;
}

pub struct Rpm {
    pub name: String,
    pub job_variables: JobVariables,
    pub source_path: String,
}

pub struct Deb {
    pub name: String,
    pub job_variables: JobVariables,
    pub source_path: String,
}

impl Package for Rpm {
    fn name(&self) -> &str {
        &self.name
    }
    fn version(&self) -> &str {
        &self.job_variables.version
    }
    fn source_path(&self) -> &str {
        &self.source_path
    }
}

impl Package for Deb {
    fn name(&self) -> &str {
        &self.name
    }
    fn version(&self) -> &str {
        &self.job_variables.version
    }
    fn source_path(&self) -> &str {
        &self.source_path
    }
}
