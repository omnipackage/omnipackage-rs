use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

mod build;
mod config;
mod distros;
mod gpg;
mod logger;
mod shell;

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    s.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).ok_or_else(|| format!("invalid KEY=VALUE: '{}'", s))
}

#[derive(Debug, Args)]
struct GlobalOpts {
    /// Container runtime, autodetect by default
    #[arg(long, global = true, value_parser = ["docker", "podman"])]
    pub container_runtime: Option<String>,
}

#[derive(Parser)]
#[command(version, about)]
struct Cli {
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

    /// Secrets passed as 'secrets' hashmap to templates and as environment variables to the container (KEY=VALUE)
    #[arg(long, short = 'e', value_parser = parse_key_val, value_name = "KEY=VALUE")]
    pub secrets: Vec<(String, String)>,
}

#[derive(Subcommand)]
enum GpgCommands {
    /// Generate a new GPG key
    Generate {
        #[arg(short, long)]
        name: String,

        #[arg(short, long)]
        email: String,

        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Build the project with omnipackage
    Build(BuildArgs),

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
        Commands::Gpg { command } => match command {
            GpgCommands::Generate { output: _, name, email } => {
                let keys = gpg::Gpg::new().generate_keys(&name, &email);
                println!("{}\n{}", keys.priv_key, keys.pub_key);

                println!(
                    "key id: {}\n{}\n{}",
                    gpg::Gpg::new().key_id(&keys.priv_key),
                    gpg::Gpg::new().key_info(&keys.priv_key),
                    gpg::Gpg::new().key_info(&keys.pub_key)
                );
            }
        },
    }
}
