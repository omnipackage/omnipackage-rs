use crate::ImageCacheRefreshArgs;
use crate::config::ImageCache;
use crate::distros::{Distro, Distros};
use crate::release;
use anyhow::{Context, Result};

pub fn refresh(args: ImageCacheRefreshArgs) -> Result<(), anyhow::Error> {
    let config = args.project.load_config(false)?;
    let ic = config.image_caches.as_ref().context("image_caches is missing")?.find_by_name_or_default(args.image_cache.as_deref())?;
    let mut any_failed = false;

    for build_config in release::detect_builds(args.job.clone(), config.clone()) {
        let distro = Distros::get().by_id(&build_config.distro);
        let ok = release::fail_fast_or_continue(refresh_distro(distro.clone(), build_config.package_name.clone(), ic.clone()), args.job.fail_fast)?;

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

fn refresh_distro(distro: Distro, package_name: String, image_cache_config: ImageCache) -> Result<(), anyhow::Error> {
    println!("{:?} -- {:?}", distro, package_name);
    Ok(())
}
