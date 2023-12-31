use log::error;
use rand::Rng;
use rand_pcg::Pcg64;
use rand_seeder::Seeder;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::{get, post, State};
use rocket_dyn_templates::Template;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::augmentation::{AugmentationClient, AugmentationPrefix};
use crate::helper::init_badges;
use crate::tree::build_tree;

#[get("/badges/<badge>")]
pub async fn badge(client: &State<Arc<AugmentationClient>>, badge: &str) -> Option<NamedFile> {
    let mut config = client.config.lock().await;

    if config.internal.last_badge_update + 3600 < chrono::Utc::now().timestamp() as u64 {
        config.internal.last_badge_update = chrono::Utc::now().timestamp() as u64;
        let _ = config.write_internal_config();
        // update badges
        drop(config);
        init_badges().await;
    }

    // assume badge exists now
    NamedFile::open(
        Path::new("static/badges/")
            .join(badge)
            .with_extension("svg"),
    )
    .await
    .ok()
}

#[get("/static/<asset..>")]
pub async fn assets(asset: PathBuf) -> Option<NamedFile> {
    NamedFile::open(Path::new("static/").join(asset)).await.ok()
}

#[get("/favicon.ico")]
pub async fn favicon() -> Option<NamedFile> {
    assets(PathBuf::from("favicon.ico")).await
}

#[get("/")]
pub async fn tree(client: &State<Arc<AugmentationClient>>) -> Template {
    let mut tree = build_tree(&client.client).await;

    // add augmentation functionality to the treeitems
    // TODO: this should really not be here, move it somewhere useful
    for augmented_channel in client.config.lock().await.internal.augmentations.iter() {
        let mut rng: Pcg64 = Seeder::from(augmented_channel.identifier.as_bytes()).make_rng();
        // generate a random color for each augmentation group
        let color = format!(
            "hsl({}, {}%, {}%)",
            rng.gen_range(0.0..360.0),
            rng.gen_range(40.0..90.0),
            rng.gen_range(60.0..80.0)
        );

        for channel in tree.channel_map.values_mut() {
            if augmented_channel.is_instance(&channel.name) {
                channel.is_augmented = true;
                channel.highlight_color = color.clone().into();
            }
        }
    }

    Template::render("index", json!({ "tree": tree }))
}

#[post("/augmentations/<name>/augment", format = "json", data = "<prefix>")]
pub async fn augment(
    client: &State<Arc<AugmentationClient>>,
    name: &str,
    prefix: Json<AugmentationPrefix>,
) -> String {
    if let Err(e) = client.add_augmentation(name, prefix.into_inner()).await {
        error!("Could not augment channel: {}", e);
        return "Error".to_string();
    }

    "Success".to_string()
}

#[post("/augmentations/<name>/remove")]
pub async fn remove(client: &State<Arc<AugmentationClient>>, name: &str) -> String {
    if (client
        .remove_augmentation(&client.get_augmentation_of_channel(name).await.unwrap())
        .await)
        .is_err()
    {
        return "Error".to_string();
    }

    "Success".to_string()
}
