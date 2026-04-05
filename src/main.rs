#![allow(dead_code)]

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};
use std::error::Error;
use std::path::{Path, PathBuf};

mod extract_version;
mod job_variables;
mod config;
mod distros;
mod gpg;
mod logger;
mod package;
mod publish;
mod release;
mod runner;
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

    /// Number of lines to print from the log on failure (only when --container-output=null)
    #[arg(long, default_value_t = 50)]
    fail_log_lines: usize,
}

#[derive(Args, Clone, Debug)]
pub struct BuildArgs {
    #[command(flatten)]
    project: ProjectArgs,

    #[command(flatten)]
    logging: LoggingArgs,

    #[command(flatten)]
    job: JobArgs,

    /// Version extractor name, if blank the first version extractor from config will be used
    #[arg(short, long)]
    version_extractor: Option<String>,
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

    /// Version extractor name, if blank the first version extractor from config will be used
    #[arg(short, long)]
    version_extractor: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct InfoArgs {
    #[command(flatten)]
    project: ProjectArgs,

    /// List all configured distros in project
    #[arg(long)]
    list_distros: bool,

    /// Show install page url
    #[arg(long)]
    show_install_page_url: bool,

    /// Repository name of the said install page, if blank the first repository from config will be used
    #[arg(short, long)]
    repository: Option<String>,

    /// Output format (list_distros only)
    #[arg(long, default_value = "plain", value_parser = ["plain", "json"])]
    format: String,
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

    /// Convert keys between pem and base64 formats
    Convert {
        /// Input file
        #[arg()]
        input: PathBuf,

        /// Format of the input key file
        #[arg(short, long, default_value = "pem")]
        input_format: String,

        #[arg(default_value = ".")]
        output_dir: PathBuf,

        /// Format of the output key file
        #[arg(short, long, default_value = "base64")]
        output_format: String,
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

    /// Query various info about the project
    Info(InfoArgs),
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    if let Some(runtime) = cli.global.container_runtime {
        shell::set_container_runtime(runtime);
    }

    match cli.command {
        Commands::Build(args) => {
            release::build(args.project, args.job, args.logging, args.version_extractor)?;
        }
        Commands::Publish(args) => {
            release::publish(args.project, args.job, args.logging, args.repository)?;
        }
        Commands::Release(args) => {
            release::release(args.project, args.job, args.logging, args.repository, args.version_extractor)?;
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
            GpgCommands::Convert {
                input,
                input_format,
                output_dir,
                output_format,
            } => {
                let content = std::fs::read(&input)?;

                let decoded = match input_format.as_str() {
                    "base64" => {
                        use base64::{Engine, engine::general_purpose};
                        general_purpose::STANDARD.decode(&content).map_err(|e| format!("Failed to decode base64 input: {}", e))?
                    }
                    _ => content,
                };

                let output_content = match output_format.as_str() {
                    "base64" => {
                        use base64::{Engine, engine::general_purpose};
                        general_purpose::STANDARD.encode(&decoded).into_bytes()
                    }
                    _ => decoded,
                };

                let input_stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("key");
                let base_name = input_stem.trim_end_matches(".base64");

                let ext = if output_format == "base64" { ".asc.base64" } else { ".asc" };
                let output_path = output_dir.join(format!("{}{}", base_name, ext));

                std::fs::write(&output_path, &output_content)?;

                println!("converted key written to {}", colorize(Color::BoldYellow, output_path.display()));
            }
        },
        Commands::Info(args) => {
            let config = args.project.load_config(true)?;
            if args.show_install_page_url {
                let repository_config = config.repositories.find_by_name_or_default(args.repository.as_deref())?.clone();
                let page_url = publish::install_page_url(&repository_config).unwrap_or("".to_string());
                println!("{}", page_url);
            } else if args.list_distros {
                let distros: Vec<&str> = config.builds.iter().map(|b| b.distro.as_str()).collect();
                match args.format.as_str() {
                    "json" => println!("{}", serde_json::to_string(&distros)?),
                    _ => distros.iter().for_each(|d| println!("{}", d)),
                }
            }
        }
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

    pub fn tail_log(&self, log_path: &Path) -> String {
        if self.container_output == "null" {
            std::fs::read_to_string(log_path)
                .map(|contents| {
                    let lines: Vec<&str> = contents.lines().collect();
                    let last = &lines[lines.len().saturating_sub(self.fail_log_lines)..];
                    format!("\n{}", last.join("\n"))
                })
                .unwrap_or_default()
        } else {
            String::new()
        }
    }
}

impl ProjectArgs {
    pub fn load_config(&self, silent: bool) -> Result<Config, String> {
        Config::load_with_env(&self.source_dir.join(&self.config_path), &self.env_file, silent)
    }
}
