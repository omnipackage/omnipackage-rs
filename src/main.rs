use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod build;
mod config;
mod logger;
mod shell;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build the project with omnipackage
    Build {
        /// Path to the project
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Distros to build
        #[arg(short, long, num_args = 0..)]
        distros: Vec<String>,

        /// Container runtime, autodetect by default
        #[arg(short, long, value_parser = ["docker", "podman"])]
        container_runtime: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { path, distros, container_runtime } => {
            if let Some(runtime) = container_runtime {
                shell::set_container_runtime(runtime);
            }

            println!("Building {:?} in {}", distros, path.display());
            println!("{:?}", config::Config::load(&path.join(".omnipackage/config.yml")));
        }
    }

    let _ = shell::Command::container(["info"]).run();

    let _ = shell::Command::new("ls").arg("-latrh").run();

    logger::info("ololo");
}
