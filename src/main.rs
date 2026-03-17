use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

mod build;
mod config;
mod distros;
mod logger;
mod shell;

#[derive(Debug, Args)]
pub struct GlobalOpts {
    /// Container runtime, autodetect by default
    #[arg(long, global = true, value_parser = ["docker", "podman"])]
    pub container_runtime: Option<String>,
}

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Args)]
pub struct BuildArgs {
    /// Path to the project
    #[arg(default_value = ".")]
    source_path: PathBuf,

    /// Distros to build, e.g. opensuse_15.6, debian_12, fedora_40, by default build for all configured distros
    #[arg(short, long, num_args = 0..)]
    distros: Vec<String>,

    /// Root directory for temporary build files
    #[arg(short, long, default_value_t = std::env::temp_dir().join("omnipackage").to_string_lossy().into_owned())]
    build_dir: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Build the project with omnipackage
    Build(BuildArgs),
}

fn main() {
    let cli = Cli::parse();

    if let Some(runtime) = cli.global.container_runtime {
        shell::set_container_runtime(runtime);
    }

    match cli.command {
        Commands::Build(args) => build::run(&args),
    }
}
