#![allow(dead_code)]
#![allow(unused)]

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
pub struct LoggingArgs {
    /// Where to print output from the containers (i.e. actual terminal output)
    #[arg(long, default_value = "stderr", value_parser = ["null", "stdout", "stderr"])]
    pub container_output: String,

    /// Disable echo (set -x) of commands inside the container
    #[arg(long)]
    pub disable_container_echo: bool,
}

#[derive(Args, Clone, Debug)]
pub struct BuildArgs {
    #[command(flatten)]
    pub project: ProjectArgs,

    #[command(flatten)]
    pub logging: LoggingArgs,

    /// Distros to build, e.g. opensuse_15.6, debian_12, fedora_40, by default build for all configured distros
    #[arg(short, long, num_args = 0..)]
    distros: Vec<String>,

    /// Root directory for temporary build files
    #[arg(long, default_value_t = default_build_dir())]
    build_dir: String,

    /// Secrets passed as 'secrets' hashmap to templates and as environment variables to the container (KEY=VALUE)
    #[arg(long, short = 's', value_parser = parse_key_val, value_name = "KEY=VALUE")]
    pub secrets: Vec<(String, String)>,
}

#[derive(Args, Clone, Debug)]
pub struct PublishArgs {
    #[command(flatten)]
    pub project: ProjectArgs,

    #[command(flatten)]
    pub logging: LoggingArgs,

    /// Distros to publish, by default pubblish all packages for all configured distros found in build_dir
    #[arg(short, long, num_args = 0..)]
    distros: Vec<String>,

    /// Root directory where previous build was executed
    #[arg(long, default_value_t = default_build_dir())]
    build_dir: String,

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
            let outputs = exit_on_error(build::run(&args));
            build::output::log_all(&outputs);
        }
        Commands::Publish(args) => {
            exit_on_error(publish::run(&args));
        }
        Commands::Gpg { command } => match command {
            GpgCommands::Generate { output_dir, name, email, format } => {
                let keys = exit_on_error(Gpg::new().generate_keys(&name, &email));

                /*let priv_path = output_dir.join("private.asc");
                let pub_path = output_dir.join("public.asc");
                // std::fs::create_dir_all(&output_dir).unwrap_or_else(|e| panic!("cannot create directory {}: {}", output_dir.display(), e));
                exit_on_error(std::fs::write(&priv_path, &keys.priv_key).map_err(|e| format!("cannot write {}: {}", priv_path.display(), e)));
                exit_on_error(std::fs::write(&pub_path, &keys.pub_key).map_err(|e| format!("cannot write {}: {}", pub_path.display(), e)));*/

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

                exit_on_error(std::fs::write(&priv_path, &priv_content).map_err(|e| format!("cannot write {}: {}", priv_path.display(), e)));
                exit_on_error(std::fs::write(&pub_path, &pub_content).map_err(|e| format!("cannot write {}: {}", pub_path.display(), e)));

                Logger::new().info(format!("private key written to {}", colorize(Color::BoldYellow, priv_path.display())));
                Logger::new().info(format!("public key written to {}", colorize(Color::BoldYellow, pub_path.display())));
            }
        },
    }
}

fn exit_on_error<T>(result: Result<T, String>) -> T {
    result.unwrap_or_else(|e| {
        Logger::new().error(e);
        std::process::exit(1);
    })
}

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    s.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).ok_or_else(|| format!("invalid KEY=VALUE: '{}'", s))
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
