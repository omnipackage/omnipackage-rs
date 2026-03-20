use crate::PublishArgs;
use crate::distros::Distro;

#[derive(Debug, Clone)]
pub struct PublishContext {
    pub distro: &'static Distro,
    args: PublishArgs,
}

pub fn run(args: &PublishArgs) {
    println!("publishing... {:?}", args);
}
