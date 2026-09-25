use crate::InfoArgs;
use crate::distros::Distros;
use crate::publish;
use anyhow::{Context, Result};

pub fn info(args: InfoArgs) -> Result<(), anyhow::Error> {
    let config = args.project.load_config(true)?;
    if args.show_install_page_url {
        let repository_config = config.repositories.find_by_name_or_default(args.repository.as_deref())?.clone();
        let page_url = publish::install_page_url(&repository_config).unwrap_or_default();
        println!("{}", page_url);
    } else if args.list_distros && args.show_images {
        let image_cache = match args.image_cache.as_deref() {
            Some(name) => Some(config.image_caches.as_ref().context("image_caches is missing")?.find_by_name_or_default(Some(name))?.clone()),
            None => None,
        };
        let images: Vec<(&str, String)> = config
            .builds
            .iter()
            .map(|b| {
                let image = match &image_cache {
                    Some(ic) => ic.full_image_name(&b.distro),
                    None => Distros::get().by_id(&b.distro).image,
                };
                (b.distro.as_str(), image)
            })
            .collect();
        match args.format.as_str() {
            "json" => {
                let json: Vec<serde_json::Value> = images.iter().map(|(d, i)| serde_json::json!({ "distro": d, "image": i })).collect();
                println!("{}", serde_json::to_string(&json)?);
            }
            _ => images.iter().for_each(|(d, i)| println!("{} {}", d, i)),
        }
    } else if args.list_distros {
        let distros: Vec<&str> = config.builds.iter().map(|b| b.distro.as_str()).collect();
        match args.format.as_str() {
            "json" => println!("{}", serde_json::to_string(&distros)?),
            _ => distros.iter().for_each(|d| println!("{}", d)),
        }
    }

    Ok(())
}
