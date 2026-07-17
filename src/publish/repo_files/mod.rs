use crate::distros::Distros;
use std::collections::HashMap;

pub mod badge;
pub mod html;
pub mod json;
pub mod sh;

pub type Repository = HashMap<String, String>;
pub type Repositories = Vec<Repository>;

pub(crate) fn upsert_repository(repositories: &mut Repositories, data: Repository) {
    let distro_id = data.get("distro_id").unwrap();

    if let Some(repo) = repositories.iter_mut().find(|repo| repo.get("distro_id").is_some_and(|value| value == distro_id)) {
        repo.extend(data);
    } else {
        repositories.push(data);
    }
}

pub(crate) fn sorted_by_distro_order(repositories: &Repositories) -> Repositories {
    let ids = Distros::get().ids();
    let mut sorted = repositories.clone();
    sorted.sort_by_key(|repo| repo.get("distro_id").and_then(|id| ids.iter().position(|d| d == id)).unwrap_or(usize::MAX));
    sorted
}
