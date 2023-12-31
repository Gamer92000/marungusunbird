use log::error;
use once_cell::sync::Lazy;
use percent_encoding::{AsciiSet, CONTROLS};
use rocket::{catchers, routes};
use rocket_dyn_templates::Template;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use ts3_query_api::event::Event;

mod augmentation;
mod badges;
mod config;
mod helper;
mod requests;
mod rocket_errors;
mod tree;

use augmentation::AugmentationClient;
use requests::{assets, augment, badge, favicon, remove, tree as tree_route};
use rocket_errors::{internal_error, not_found};

pub const FRAGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'\'');

pub static SPACER_PREFIX: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"^\[c?spacer\]\s*").unwrap());

#[tokio::main]
async fn main() {
    env_logger::init();

    let client = match AugmentationClient::new().await {
        Ok(client) => client,
        Err(e) => {
            error!("Could not connect to server: {}", e);
            return;
        }
    };
    let event_client = Arc::new(client);
    let managed_client = event_client.clone();

    tokio::spawn(async move {
        while let Ok(event) = event_client.client.wait_for_event().await {
            match event {
                Event::ClientMoved(_) | Event::ClientEnterView(_) | Event::ClientLeftView(_) => {
                    event_client.update_augmented_channels().await.unwrap();
                }
                _ => {}
            }
        }
    });

    let addr = managed_client
        .config
        .lock()
        .await
        .external
        .bind_addr
        .clone();
    let port = managed_client.config.lock().await.external.bind_port;

    fn extract_spacer_name(
        value: &Value,
        _args: &HashMap<String, Value>,
    ) -> Result<Value, rocket_dyn_templates::tera::Error> {
        Ok(SPACER_PREFIX.replace(value.as_str().unwrap(), "").into())
    }

    // start web server
    let _ = rocket::build()
        .configure(
            rocket::Config::figment()
                .merge(("port", port))
                .merge(("address", &addr)),
        )
        .manage(managed_client)
        .attach(Template::custom(|engines| {
            // Add your custom filter to the Tera instance
            engines
                .tera
                .register_filter("extract_spacer_name", extract_spacer_name);
        }))
        .mount(
            "/",
            routes![assets, augment, remove, tree_route, favicon, badge],
        )
        .register("/", catchers![internal_error, not_found])
        .launch()
        .await;
}
