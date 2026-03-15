use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod config;

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
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { path, distros } => {
            println!("Building {:?} in {}", distros, path.display());
            println!("{:?}", config::Config::load(&path.join(".omnipackage/config.yml")));
        }
    }
}
