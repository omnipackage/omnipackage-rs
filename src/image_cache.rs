use crate::ImageCacheRefreshArgs;
use crate::config::{Build, ImageCache, ImageCacheProvider};
use crate::distros::Distros;
use crate::logger::Logger;
use crate::release;
use crate::shell::Command;
use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn refresh(args: ImageCacheRefreshArgs) -> Result<(), anyhow::Error> {
    let config = args.project.load_config(false)?;
    let ic = config
        .image_caches
        .as_ref()
        .context("image_caches is missing")?
        .find_by_name_or_default(args.job.image_cache.as_deref())?;
    let mut any_failed = false;

    for build_config in release::detect_builds(args.job.clone(), config.clone()) {
        Logger::new().info(format!("starting image cache refresh for {}", build_config.distro));
        let res = refresh_distro(args.clone(), build_config.clone(), ic.clone());
        if let Err(ref e) = res {
            Logger::new().error(format!("image cache refresh error: {}", e));
        } else {
            Logger::new().info(format!("finished image cache refresh for {}", build_config.distro));
        }

        let ok = release::fail_fast_or_continue(res, args.job.fail_fast)?;

        if !ok {
            any_failed = true;
        }
    }

    if any_failed {
        Err(anyhow::anyhow!("image cache refresh for one or more distros failed"))
    } else {
        Ok(())
    }
}

pub fn login_to_registry(image_cache_config: ImageCache, logger: Logger, log_path: Option<&PathBuf>) -> Result<(), anyhow::Error> {
    let registry = image_cache_config.registry.clone().context("registry config is missing")?;

    let mut cmd = Command::container(vec![
        "login".to_string(),
        registry.url.clone(),
        "-u".to_string(),
        registry.username.clone(),
        "--password-stdin".to_string(),
    ])
    .stream_output_to(logger)
    .with_stdin(move |stdin| {
        stdin.write_all(registry.password.as_bytes()).unwrap();
    });
    if let Some(v) = log_path {
        cmd = cmd.log_to(v);
    }
    cmd.run()
}

fn refresh_distro(args: ImageCacheRefreshArgs, build_config: Build, image_cache_config: ImageCache) -> Result<(), anyhow::Error> {
    let distro = Distros::get().by_id(&build_config.distro);
    let temp_dir = args.job.build_dir.join(format!("{}-{}", build_config.package_name, build_config.distro));

    let base_image = distro.image.clone();
    let mut commands: Vec<String> = Vec::new();
    commands.extend(distro.setup(&build_config.build_dependencies));
    commands.extend(distro.setup_repo.clone());

    let mut has_bbs_file = false;
    if let Some(bbs) = build_config.before_build_script {
        let bbs_path = args.project.source_dir.join(&bbs);
        if bbs_path.exists() {
            has_bbs_file = true;
            std::fs::copy(&bbs_path, temp_dir.join("before_build_script"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let dest = temp_dir.join("before_build_script");
                std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
            }
            commands.push("/before_build_script".to_string());
        } else {
            commands.push(bbs.to_string());
        };
    }
    commands.extend(distro.cleanup.clone());

    let mut runcmd = String::new();
    if has_bbs_file {
        runcmd.push_str("--mount=type=bind,source=before_build_script,target=/before_build_script \\");
    }
    runcmd.push_str(&commands.join(" && "));

    let dockerfile = format!(
        "FROM {base_image}\n\
         RUN {runcmd}\n"
    );
    std::fs::create_dir_all(&temp_dir)?;
    std::fs::write(temp_dir.join("Dockerfile"), &dockerfile)?;

    let output_image = image_cache_config.full_image_name(&distro.id);
    let cliargs = vec!["build".to_string(), "-t".to_string(), output_image.clone(), ".".to_string()]; // "--no-cache".to_string()

    Command::container(cliargs).stream_output_to(args.logging.container_logger()).current_dir(temp_dir.clone()).run()?;

    if image_cache_config.provider == ImageCacheProvider::Registry {
        Logger::new().info(format!("pushing image {} to registry", output_image));
        login_to_registry(image_cache_config.clone(), args.logging.container_logger(), None)?;
        Command::container(vec!["push".to_string(), output_image.clone()])
            .stream_output_to(args.logging.container_logger())
            .current_dir(temp_dir.clone())
            .run()?;
    }

    Ok(())
}
