#![allow(dead_code)]

use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod build;
mod config;
mod distros;
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

        /// Distros to build, e.g. opensuse_15.6, debian_12, fedora_40, by default build for all configured distros
        #[arg(short, long, num_args = 0..)]
        distros: Vec<String>,

        /// Container runtime, autodetect by default
        #[arg(short, long, value_parser = ["docker", "podman"])]
        container_runtime: Option<String>,

        /// Root directory for temporary build files
        #[arg(short, long, default_value_t = std::env::temp_dir().to_string_lossy().into_owned())]
        build_dir: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build {
            path,
            distros,
            container_runtime,
            build_dir,
        } => {
            if let Some(runtime) = container_runtime {
                shell::set_container_runtime(runtime);
            }

            build::run(distros, path, PathBuf::from(&build_dir));

            //println!("Building {:?} in {}", distros, path.display());
            //println!("{:?}", config::Config::load(&path.join(".omnipackage/config.yml")));

            /*let all = distros::Distros::get();
            let distros_to_build: Vec<&distros::Distro> = if distros.is_empty() {
                all.distros.iter().collect()
            } else {
                distros.iter().map(|id| all.by_id(id)).collect()
            };

            for distro in distros_to_build {
                let build = build::BuildContext {
                    distro: distro,
                    path: path.clone(),
                };
                build.run();
            }*/
        }
    }

    let _ = shell::Command::container(["run", "--rm", "opensuse/tumbleweed", "sh", "-c", "for i in $(seq 1 10); do echo $i; sleep 1; done"])
        .log_to("/tmp/build.log")
        .stream_output_to(shell::StreamOutput::Stderr)
        .run();

    let _ = shell::Command::new("ls").arg("-latrh").run();

    logger::info("ololo");
}
