#![allow(dead_code)]

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

mod build;
mod config;
mod distros;
mod gpg;
mod logger;
mod publish;
mod shell;

use gpg::Gpg;
use logger::{Color, Logger, colorize};

#[derive(Debug, Args)]
struct GlobalOpts {
    /// Container runtime, autodetect by default
    #[arg(long, global = true, value_parser = ["docker", "podman"])]
    pub container_runtime: Option<String>,
}

#[derive(Parser)]
#[command(version, about)]
#[command(styles = styles())]
struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Args, Clone, Debug)]
pub struct ProjectArgs {
    /// Path to the project
    #[arg(default_value = ".")]
    pub source_dir: PathBuf,

    /// Relative path within source_dir to the omnipackage config
    #[arg(long, default_value = ".omnipackage/config.yml")]
    pub config_path: PathBuf,

    /// Full path to .env file containing secrets rendered in config
    #[arg(long, default_value = ".env")]
    pub env_file: PathBuf,
}

#[derive(Args, Clone, Debug)]
pub struct BuildArgs {
    #[command(flatten)]
    pub project: ProjectArgs,

    /// Distros to build, e.g. opensuse_15.6, debian_12, fedora_40, by default build for all configured distros
    #[arg(short, long, num_args = 0..)]
    distros: Vec<String>,

    /// Root directory for temporary build files
    #[arg(long, default_value_t = default_build_dir())]
    build_dir: String,

    /// Secrets passed as 'secrets' hashmap to templates and as environment variables to the container (KEY=VALUE)
    #[arg(long, short = 's', value_parser = parse_key_val, value_name = "KEY=VALUE")]
    pub secrets: Vec<(String, String)>,

    /// Where to print output from the containers (i.e. actual build terminal output)
    #[arg(long, default_value = "stderr", value_parser = ["null", "stdout", "stderr"])]
    pub container_output: String,

    /// Disale echo (set -x) of commands inside the container
    #[arg(long)]
    pub disable_container_echo: bool,
}

/*#[derive(Args, Clone, Debug)]
pub struct DistroArtefacts {
    /// Distro id, e.g. opensuse_15.6, debian_12, fedora_40
    #[arg(short, long)]
    pub distro: String,

    /// Artefacts, i.e. RPMs or DEBs to publish
    #[arg(short, long)]
    pub artefacts: Vec<PathBuf>,
}*/

#[derive(Args, Clone, Debug)]
pub struct PublishArgs {
    #[command(flatten)]
    pub project: ProjectArgs,

    /// Distros to publish, by default pubblish all packages for all configured distros found in build_dir
    #[arg(short, long, num_args = 0..)]
    distros: Vec<String>,

    /// Root directory where previous build was executed
    #[arg(long, default_value_t = default_build_dir())]
    build_dir: String,

    /*/// Distro and artefacts in format DISTRO:PATH1,PATH2
    #[arg(short, long, value_parser = parse_distro_artefacts, required = true, num_args = 1..)]
    pub artefacts: Vec<DistroArtefacts>,*/
    /// Repository name, if blank the first repository from config will be used
    #[arg(short, long)]
    repository: Option<String>,
}

#[derive(Subcommand)]
enum GpgCommands {
    /// Generate a new GPG key
    Generate {
        #[arg(default_value = ".")]
        output_dir: PathBuf,

        #[arg(short, long)]
        name: String,

        #[arg(short, long)]
        email: String,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Build the project with omnipackage
    Build(BuildArgs),

    /// Publish built artefacts to a repository
    Publish(PublishArgs),

    /// GPG key management
    Gpg {
        #[command(subcommand)]
        command: GpgCommands,
    },
}

fn main() {
    let cli = Cli::parse();

    if let Some(runtime) = cli.global.container_runtime {
        shell::set_container_runtime(runtime);
    }

    match cli.command {
        Commands::Build(args) => {
            let outputs = build::run(&args);
            build::output::log_all(&outputs);
        }
        Commands::Publish(args) => {
            publish::run(&args);
        }
        Commands::Gpg { command } => match command {
            GpgCommands::Generate { output_dir, name, email } => {
                let keys = Gpg::new().generate_keys(&name, &email);

                let priv_path = output_dir.join("private.asc");
                let pub_path = output_dir.join("public.asc");
                // std::fs::create_dir_all(&output_dir).unwrap_or_else(|e| panic!("cannot create directory {}: {}", output_dir.display(), e));
                std::fs::write(&priv_path, &keys.priv_key).unwrap_or_else(|e| panic!("cannot write {}: {}", priv_path.display(), e));
                std::fs::write(&pub_path, &keys.pub_key).unwrap_or_else(|e| panic!("cannot write {}: {}", pub_path.display(), e));

                Logger::new().info(format!("private key written to {}", colorize(Color::BoldYellow, priv_path.display())));
                Logger::new().info(format!("public key written to {}", colorize(Color::BoldYellow, pub_path.display())));
            }
        },
    }
}

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    s.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).ok_or_else(|| format!("invalid KEY=VALUE: '{}'", s))
}

/*fn parse_distro_artefacts(s: &str) -> Result<DistroArtefacts, String> {
    let (distro, paths) = s.split_once(':').ok_or_else(|| format!("invalid format, expected DISTRO:PATH1,PATH2: '{}'", s))?;

    let artefacts = paths.split(',').map(PathBuf::from).collect();

    Ok(DistroArtefacts {
        distro: distro.to_string(),
        artefacts,
    })
}*/

fn default_build_dir() -> String {
    std::env::temp_dir().join("omnipackage").to_string_lossy().into()
}

fn styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Green.on_default() | Effects::BOLD)
        .usage(AnsiColor::Green.on_default() | Effects::BOLD)
        .literal(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Cyan.on_default())
        .error(AnsiColor::Red.on_default() | Effects::BOLD)
        .valid(AnsiColor::Green.on_default() | Effects::BOLD)
        .invalid(AnsiColor::Yellow.on_default() | Effects::BOLD)
}

impl Default for BuildArgs {
    fn default() -> Self {
        #[derive(Parser)]
        struct Dummy {
            #[command(flatten)]
            args: BuildArgs,
        }
        Dummy::parse_from(["dummy"]).args
    }
}
