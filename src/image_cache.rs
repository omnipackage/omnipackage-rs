use crate::ImageCacheRefreshArgs;
use crate::config::{Build, ImageCache};
use crate::distros::Distros;
use crate::logger::Logger;
use crate::release;
use crate::shell::Command;
use anyhow::{Context, Result};

pub fn refresh(args: ImageCacheRefreshArgs) -> Result<(), anyhow::Error> {
    let config = args.project.load_config(false)?;
    let ic = config.image_caches.as_ref().context("image_caches is missing")?.find_by_name_or_default(args.image_cache.as_deref())?;
    let mut any_failed = false;

    for build_config in release::detect_builds(args.job.clone(), config.clone()) {
        let res = refresh_distro(args.clone(), build_config.clone(), ic.clone());
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

fn refresh_distro(args: ImageCacheRefreshArgs, build_config: Build, image_cache_config: ImageCache) -> Result<(), anyhow::Error> {
    let distro = Distros::get().by_id(&build_config.distro);
    let temp_dir = args.job.build_dir.join(format!("{}-{}", build_config.package_name, build_config.distro));

    let base_image = distro.image.clone();
    let mut commands: Vec<String> = Vec::new();
    commands.extend(distro.setup(&build_config.build_dependencies));
    commands.extend(distro.setup_repo.clone());
    let runcmd = commands.join(" && ");
    let dockerfile = format!(
        "FROM {base_image}\n\
         RUN {runcmd}\n"
    );
    std::fs::create_dir_all(&temp_dir)?;
    std::fs::write(temp_dir.join("Dockerfile"), &dockerfile)?;

    let output_image = format!("{}:{}", distro.id, image_cache_config.image_tag.unwrap_or_else(|| build_config.package_name.clone()));
    let cliargs = vec!["build".to_string(), "-t".to_string(), output_image, ".".to_string()];

    Command::container(cliargs).stream_output_to(Logger::new()).current_dir(temp_dir).run()
}
