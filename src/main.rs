use env_logger::{fmt::Color, Builder, Env};
use helper::extract_spacer_name;
use lazy_static::lazy_static;
use log::{error, info};
use percent_encoding::{AsciiSet, CONTROLS};
use regex::Regex;
use rocket::{catchers, routes};
use rocket_dyn_templates::Template;
use std::io::Write;
use std::sync::Arc;
use ts3_query_api::event::Event;

mod augmentation;
mod badges;
mod config;
mod errors;
mod helper;
mod requests;
mod rocket_errors;
mod tree;

use augmentation::AugmentationClient;
use requests::{
    abridge, assets, augment, augmentation as augmentation_route, badge, change_prefix, channel,
    client, favicon, tree as tree_route,
};
use rocket_errors::{internal_error, not_found};

use crate::helper::base64_encode;

pub const FRAGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'\'');

#[tokio::main]
async fn main() {
    let mut builder = Builder::from_env(Env::default().filter_or("LOG_LEVEL", "info,rocket::server=warn,ts3_query_api::protocol=debug,rocket_dyn_templates=info,rocket::shield=off,rocket::launch=off"));
    builder
        .format(|buf, record| {
            lazy_static! {
                static ref PASSWORD_REGEX: Regex =
                    Regex::new(r"client_login_password=[^\s]*").unwrap();
            }
            let mut message = format!("{}", record.args());
            message = PASSWORD_REGEX
                .replace_all(&message, "client_login_password=<redacted>")
                .to_string();
            writeln!(
                buf,
                "[{} {:<5} {}] {}",
                buf.style()
                    .set_color(Color::Black)
                    .set_intense(true)
                    .value(chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
                buf.default_styled_level(record.level()),
                buf.style()
                    .set_color(Color::Black)
                    .set_intense(true)
                    .value(record.target()),
                message
            )
        })
        .init();

    info!("Starting up");

    let client = match AugmentationClient::new().await {
        Ok(client) => client,
        Err(e) => {
            error!("Could not connect to server: {}", e);
            return;
        }
    };
    let event_client = Arc::new(client);
    let managed_client = event_client.clone();

    info!("Successfully connected to TeamSpeak server query");

    tokio::spawn(async move {
        while let Ok(event) = event_client.client.wait_for_event().await {
            match event {
                Event::ClientMoved(_) | Event::ClientEnterView(_) | Event::ClientLeftView(_) => {
                    match event_client.update_augmented_channels().await {
                        Ok(_) => {}
                        Err(e) => {
                            error!("Could not update augmented channels: {}", e);
                        }
                    }
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

    info!("Starting web server on {}:{}", addr, port);

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
            engines.tera.register_filter("base64_encode", base64_encode);
        }))
        .mount(
            "/",
            routes![
                augmentation_route,
                assets,
                augment,
                abridge,
                tree_route,
                favicon,
                badge,
                channel,
                client,
                change_prefix
            ],
        )
        .register("/", catchers![internal_error, not_found])
        .launch()
        .await;
}
