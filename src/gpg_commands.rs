use crate::gpg::Gpg;
use crate::logger::{Color, colorize};
use crate::{GpgConvertArgs, GpgGenerateArgs};
use anyhow::{Context, Result};

pub fn generate(args: GpgGenerateArgs) -> Result<(), anyhow::Error> {
    let keys = Gpg::new().generate_keys(&args.name, &args.email)?;

    let (priv_content, pub_content) = match args.format.as_str() {
        "base64" => {
            use base64::{Engine, engine::general_purpose};
            (general_purpose::STANDARD.encode(&keys.priv_key), general_purpose::STANDARD.encode(&keys.pub_key))
        }
        _ => (keys.priv_key.clone(), keys.pub_key.clone()),
    };

    let ext = if args.format == "base64" { ".base64" } else { "" };
    let priv_path = args.output_dir.join(format!("private.asc{}", ext));
    let pub_path = args.output_dir.join(format!("public.asc{}", ext));

    std::fs::write(&priv_path, &priv_content)?;
    std::fs::write(&pub_path, &pub_content)?;

    println!("private key written to {}", colorize(Color::BoldYellow, priv_path.display()));
    println!("public key written to {}", colorize(Color::BoldYellow, pub_path.display()));

    Ok(())
}

pub fn convert(args: GpgConvertArgs) -> Result<(), anyhow::Error> {
    let content = std::fs::read(&args.input)?;

    let decoded = match args.input_format.as_str() {
        "base64" => {
            use base64::{Engine, engine::general_purpose};
            general_purpose::STANDARD.decode(&content).with_context(|| "Failed to decode base64 input".to_string())?
        }
        _ => content,
    };

    let output_content = match args.output_format.as_str() {
        "base64" => {
            use base64::{Engine, engine::general_purpose};
            general_purpose::STANDARD.encode(&decoded).into_bytes()
        }
        _ => decoded,
    };

    let input_stem = args.input.file_stem().and_then(|s| s.to_str()).unwrap_or("key");
    let base_name = input_stem.trim_end_matches(".base64");

    let ext = if args.output_format == "base64" { ".asc.base64" } else { ".asc" };
    let output_path = args.output_dir.join(format!("{}{}", base_name, ext));

    std::fs::write(&output_path, &output_content)?;

    println!("converted key written to {}", colorize(Color::BoldYellow, output_path.display()));

    Ok(())
}
