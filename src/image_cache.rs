use crate::ImageCacheRefreshArgs;
use crate::config::ImageCache;
use anyhow::Result;

pub fn refresh(args: ImageCacheRefreshArgs) -> Result<(), anyhow::Error> {
    let config = args.project.load_config(false)?;
    println!("{:?}\n\n{:?}\n", args, config);
    Ok(())
}
