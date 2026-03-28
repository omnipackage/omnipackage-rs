#![allow(dead_code)]

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};
use std::error::Error;
use std::path::PathBuf;

mod artefacts;
mod build;
mod config;
mod distros;
mod gpg;
mod logger;
mod publish;
mod release;
mod shell;
mod template;

use config::Config;
use gpg::Gpg;
use logger::{Color, LogOutput, Logger, colorize};

#[derive(Debug, Args)]
struct GlobalOpts {
    /// Container runtime, autodetect by default
    #[arg(long, global = true, value_parser = ["docker", "podman"])]
    container_runtime: Option<String>,
}

#[derive(Parser)]
#[command(version, about)]
#[command(styles = styles())]
struct Cli {
    #[command(flatten)]
    global: GlobalOpts,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Args, Clone, Debug)]
pub struct ProjectArgs {
    /// Path to the project
    #[arg(default_value = ".")]
    source_dir: PathBuf,

    /// Relative path within source_dir to the omnipackage config
    #[arg(long, default_value = ".omnipackage/config.yml")]
    config_path: PathBuf,

    /// Full path to .env file containing secrets rendered in config
    #[arg(long, default_value = ".env")]
    env_file: PathBuf,
}

#[derive(Args, Clone, Debug)]
pub struct JobArgs {
    /// Distros to build/publish, e.g. opensuse_15.6, debian_12, fedora_40, by default build for all configured distros
    #[arg(short, long, num_args = 0..)]
    distros: Vec<String>,

    /// Root directory for temporary build/publish files
    #[arg(long, default_value_t = default_build_dir())]
    build_dir: String,

    /// Stop on first error instead of continuing with remaining distros
    #[arg(long, default_value_t = false)]
    fail_fast: bool,
}

#[derive(Args, Clone, Debug)]
pub struct LoggingArgs {
    /// Where to print output from the containers (i.e. actual terminal output)
    #[arg(long, default_value = "stderr", value_parser = ["null", "stdout", "stderr"])]
    container_output: String,

    /// Disable echo (set -x) of commands inside the container
    #[arg(long)]
    disable_container_echo: bool,
}

#[derive(Args, Clone, Debug)]
pub struct BuildArgs {
    #[command(flatten)]
    project: ProjectArgs,

    #[command(flatten)]
    logging: LoggingArgs,

    #[command(flatten)]
    job: JobArgs,
}

#[derive(Args, Clone, Debug)]
pub struct PublishArgs {
    #[command(flatten)]
    project: ProjectArgs,

    #[command(flatten)]
    logging: LoggingArgs,

    #[command(flatten)]
    job: JobArgs,

    /// Repository name, if blank the first repository from config will be used
    #[arg(short, long)]
    repository: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct ReleaseArgs {
    #[command(flatten)]
    project: ProjectArgs,

    #[command(flatten)]
    logging: LoggingArgs,

    #[command(flatten)]
    job: JobArgs,

    /// Repository name to publish to, if blank the first repository from config will be used
    #[arg(short, long)]
    repository: Option<String>,
}

#[derive(Subcommand)]
enum GpgCommands {
    /// Generate a new GPG key
    Generate {
        #[arg(default_value = ".")]
        output_dir: PathBuf,

        /// Key owner name, i.e. your real name
        #[arg(short, long)]
        name: String,

        /// Key owner email, i.e. your real email
        #[arg(short, long)]
        email: String,

        /// Output format
        #[arg(long, default_value = "pem", value_parser = ["pem", "base64"])]
        format: String,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Build the project with omnipackage
    Build(BuildArgs),

    /// Publish built artefacts to a repository
    Publish(PublishArgs),

    /// Build and publish in one go
    Release(ReleaseArgs),

    /// GPG key management
    Gpg {
        #[command(subcommand)]
        command: GpgCommands,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    if let Some(runtime) = cli.global.container_runtime {
        shell::set_container_runtime(runtime);
    }

    match cli.command {
        Commands::Build(args) => {
            release::build(args.project, args.job, args.logging)?;
        }
        Commands::Publish(args) => {
            release::publish(args.project, args.job, args.logging, args.repository)?;
        }
        Commands::Release(args) => {
            release::release(args.project, args.job, args.logging, args.repository)?;
        }
        Commands::Gpg { command } => match command {
            GpgCommands::Generate { output_dir, name, email, format } => {
                let keys = Gpg::new().generate_keys(&name, &email)?;

                let (priv_content, pub_content) = match format.as_str() {
                    "base64" => {
                        use base64::{Engine, engine::general_purpose};
                        (general_purpose::STANDARD.encode(&keys.priv_key), general_purpose::STANDARD.encode(&keys.pub_key))
                    }
                    _ => (keys.priv_key.clone(), keys.pub_key.clone()),
                };

                let ext = if format == "base64" { ".base64" } else { "" };
                let priv_path = output_dir.join(format!("private.asc{}", ext));
                let pub_path = output_dir.join(format!("public.asc{}", ext));

                std::fs::write(&priv_path, &priv_content)?;
                std::fs::write(&pub_path, &pub_content)?;

                println!("private key written to {}", colorize(Color::BoldYellow, priv_path.display()));
                println!("public key written to {}", colorize(Color::BoldYellow, pub_path.display()));
            }
        },
    }
    Ok(())
}

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

impl LoggingArgs {
    pub fn container_logger(&self) -> Logger {
        let output = match self.container_output.as_str() {
            "stderr" => LogOutput::Stderr,
            "stdout" => LogOutput::Stdout,
            _ => LogOutput::Silent,
        };
        Logger::new().with_output(output)
    }
}

impl ProjectArgs {
    pub fn load_config(&self) -> Result<Config, String> {
        Config::load_with_env(&self.source_dir.join(&self.config_path), &self.env_file)
    }
}
