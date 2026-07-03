use base64::{engine::general_purpose, Engine as _};
use chrono::prelude::DateTime;
use chrono::Utc;
use log::{error, info};
use rocket::futures::{SinkExt, StreamExt};
use rocket::fs::NamedFile;
use rocket::response::Redirect;
use rocket::serde::json::Json;
use rocket::{get, post, State};
use rocket_dyn_templates::Template;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::sync::broadcast::error::RecvError;
use ts3_query_api::definitions::Codec;

use crate::augmentation::{AugmentationClient, AugmentationPrefix};
use crate::helper::{format_duration, init_badges};
use crate::live::render_tree_html;
use crate::tree::build_tree;

// ===============
// ASSET endpoints
// ===============

/// Build the JSON snapshot pushed to WebSocket clients: current connection state
/// plus the rendered tree HTML (empty while disconnected).
async fn tree_snapshot(client: &AugmentationClient) -> String {
    let (connected, tree_html) = if client.is_connected() {
        let config = client.config.lock().await;
        let built = build_tree(&*client.client.read().await, &config.internal.augmentations).await;
        drop(config);
        match built {
            Ok(tree) => match render_tree_html(&tree) {
                Ok(html) => (true, html),
                Err(e) => {
                    error!("Could not render tree fragment: {e}");
                    (true, String::new())
                }
            },
            // Reaching the server failed even though the flag says connected —
            // treat it as down for display purposes.
            Err(_) => (false, String::new()),
        }
    } else {
        (false, String::new())
    };

    json!({ "connected": connected, "tree_html": tree_html }).to_string()
}

/// Live updates over a WebSocket: pushes a fresh snapshot on connect and whenever
/// the tree or connection state changes. Replaces client-side polling.
#[get("/ws")]
pub fn ws(ws: rocket_ws::WebSocket, client: &State<Arc<AugmentationClient>>) -> rocket_ws::Channel<'static> {
    let client = client.inner().clone();
    ws.channel(move |mut stream| {
        Box::pin(async move {
            let mut updates = client.subscribe_updates();

            // Initial snapshot so a freshly opened page renders immediately.
            if stream
                .send(rocket_ws::Message::Text(tree_snapshot(&client).await))
                .await
                .is_err()
            {
                return Ok(());
            }

            loop {
                tokio::select! {
                    // Client -> server: only care about close/errors to end the loop.
                    incoming = stream.next() => match incoming {
                        Some(Ok(rocket_ws::Message::Close(_))) | None => break,
                        Some(Err(_)) => break,
                        Some(Ok(_)) => {}
                    },
                    // Change notification -> push a new snapshot.
                    recv = updates.recv() => match recv {
                        Ok(()) | Err(RecvError::Lagged(_)) => {
                            if stream
                                .send(rocket_ws::Message::Text(tree_snapshot(&client).await))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(RecvError::Closed) => break,
                    },
                }
            }

            Ok(())
        })
    })
}

#[get("/badges/<badge>")]
pub async fn badge(client: &State<Arc<AugmentationClient>>, badge: &str) -> Option<NamedFile> {
    let mut config = client.config.lock().await;

    if config.internal.last_badge_update + 3600 < chrono::Utc::now().timestamp() as u64 {
        config.internal.last_badge_update = chrono::Utc::now().timestamp() as u64;
        let _ = config.write_internal_config();
        // update badges
        drop(config);

        info!("Updating badges");

        // init badges, but don't wait for it
        tokio::spawn(async move {
            if let Err(e) = init_badges().await {
                error!("Could not update badges: {e}");
            }
        });
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

// ============
// UI endpoints
// ============

#[get("/")]
pub async fn tree(client: &State<Arc<AugmentationClient>>) -> Template {
    // While the query connection is down, render the page with a banner instead of
    // trying to reach the server (which would 500).
    if !client.is_connected() {
        return Template::render(
            "index",
            json!({ "connected": false, "name": "", "properties": [] }),
        );
    }

    let config = client.config.lock().await;
    let tree = match build_tree(&*client.client.read().await, &config.internal.augmentations).await {
        Ok(tree) => tree,
        Err(e) => {
            error!("Could not build tree: {e}");
            drop(config);
            return Template::render(
                "index",
                json!({ "connected": false, "name": "", "properties": [] }),
            );
        }
    };
    drop(config);

    let server = client.client.read().await.server_info().await.unwrap();

    let created = UNIX_EPOCH + Duration::from_secs(server.created);
    // Create DateTime from SystemTime
    let created = DateTime::<Utc>::from(created);
    // Formats the combined date and time with the specified format string.
    let created = created.format("%d.%m.%Y %H:%M").to_string();

    Template::render(
        "index",
        json!({
            "connected": true,
            "tree": tree,
            "name": server.name.to_string(),
            "properties": [
                {"name": "VS ID", "value": server.id},
                {"name": "Welcome Message", "value": server.welcome_message},
                {"name": "Created", "value": created},
                {"name": "Uptime", "value": format_duration(server.uptime as i64)},
                {"name": "Max Clients", "value": server.max_clients},
                {"name": "Listen", "value": server
                .ip
                .split(", ")
                .map(|ip| {
                    if ip.contains(':') {
                        format!("[{}]:{}", ip, server.port)
                    } else {
                        format!("{}:{}", ip, server.port)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")},
                {"name": "Version", "value": server.version},
                {"name": "Platform", "value": server.platform},
                {"name": "Capabilities", "value": server.capability_extensions}
            ],
        }),
    )
}

#[get("/channel/<id>")]
pub async fn channel(
    client: &State<Arc<AugmentationClient>>,
    id: i32,
) -> Result<Template, Redirect> {
    let config = client.config.lock().await;
    let tree = match build_tree(&*client.client.read().await, &config.internal.augmentations).await {
        Ok(tree) => tree,
        Err(e) => {
            // Connection down — send the user to the index, which shows the banner.
            error!("Could not build tree: {e}");
            return Err(Redirect::to("/"));
        }
    };
    let channel = match client.client.read().await.channel_info(id).await {
        Ok(channel) => channel,
        Err(_) => return Err(Redirect::to("/")),
    };

    // check if channel is augmented, if so redirect to augmentation
    if let Some(augmentation) = config
        .internal
        .augmentations
        .iter()
        .find(|a| a.is_instance(&channel.name))
    {
        let redirection = format!(
            "/augmentation/{}",
            general_purpose::URL_SAFE_NO_PAD.encode(&augmentation.identifier)
        );
        info!("Redirecting to {redirection}");
        return Err(Redirect::to(redirection));
    }

    drop(config);

    Ok(Template::render(
        "channel",
        json!({
            "connected": true,
            "tree": tree,
            "properties": [
                {"name": "Topic", "value": channel.topic},
                {"name": "Description", "value": channel.description},
                {"name": "Codec", "value": match channel.codec {
                    Codec::SpeexNarrowband => "Speex Narrowband",
                    Codec::SpeexWideband => "Speex Wideband",
                    Codec::SpeexUltraWideband => "Speex Ultra-Wideband",
                    Codec::Celt => "CELT",
                    Codec::OpusVoice => "Opus Voice",
                    Codec::OpusMusic => "Opus Music",
                    _ => "Unknown",
                }},
                {"name": "Codec Quality", "value": channel.codec_quality},
                {"name": "Max Clients", "value": match channel.max_clients {
                    -1 => "Unlimited".to_string(),
                    _ => channel.max_clients.to_string(),
                }},
                {"name": "Max Family Clients", "value": match channel.max_family_clients {
                    -1 => "Unlimited".to_string(),
                    _ => channel.max_family_clients.to_string(),
                }},
                {"name": "Needed Talk Power", "value": channel.needed_talk_power},
            ],
            "name": channel.name,
            "id": id,
        }),
    ))
}

#[get("/augmentation/<name>")]
pub async fn augmentation(
    client: &State<Arc<AugmentationClient>>,
    name: String,
) -> Result<Template, Redirect> {
    let name = match String::from_utf8(
        match general_purpose::URL_SAFE_NO_PAD.decode(name.as_bytes()) {
            Ok(name) => name,
            Err(e) => {
                error!("Could not decode augmentation name: {e}");
                return Err(Redirect::to("/"));
            }
        },
    ) {
        Ok(name) => name,
        Err(e) => {
            error!("Could not decode augmentation name: {e}");
            return Err(Redirect::to("/"));
        }
    };

    let config = client.config.lock().await;
    let tree = match build_tree(&*client.client.read().await, &config.internal.augmentations).await {
        Ok(tree) => tree,
        Err(e) => {
            // Connection down — send the user to the index, which shows the banner.
            error!("Could not build tree: {e}");
            return Err(Redirect::to("/"));
        }
    };
    // get data for augmentation
    let augmentation = match config
        .internal
        .augmentations
        .iter()
        .find(|a| a.identifier == name)
    {
        Some(a) => a,
        None => {
            // check if channel with name exists
            if let Some(channel) = client
                .client
                .read()
                .await
                .channel_list()
                .await
                .unwrap()
                .iter()
                .find(|c| c.name == name)
            {
                return Err(Redirect::to(format!("/channel/{}", channel.id)));
            }
            return Err(Redirect::to("/"));
        }
    };
    // find first channel of augmentation
    let channel = match client
        .client
        .read()
        .await
        .channel_list()
        .await
        .unwrap()
        .iter()
        .find(|c| augmentation.is_instance(&c.name))
    {
        Some(c) => c.id,
        None => return Err(Redirect::to("/")),
    };
    let channel = match client.client.read().await.channel_info(channel).await {
        Ok(channel) => channel,
        Err(_) => return Err(Redirect::to("/")),
    };

    Ok(Template::render(
        "augmentation",
        json!({
            "connected": true,
            "tree": tree,
            "properties": [
                {"name": "Topic", "value": channel.topic},
                {"name": "Description", "value": channel.description},
                {"name": "Codec", "value": match channel.codec {
                    Codec::SpeexNarrowband => "Speex Narrowband",
                    Codec::SpeexWideband => "Speex Wideband",
                    Codec::SpeexUltraWideband => "Speex Ultra-Wideband",
                    Codec::Celt => "CELT",
                    Codec::OpusVoice => "Opus Voice",
                    Codec::OpusMusic => "Opus Music",
                    _ => "Unknown",
                }},
                {"name": "Codec Quality", "value": channel.codec_quality},
                {"name": "Max Clients", "value": match channel.max_clients {
                    -1 => "Unlimited".to_string(),
                    _ => channel.max_clients.to_string(),
                }},
                {"name": "Max Family Clients", "value": match channel.max_family_clients {
                    -1 => "Unlimited".to_string(),
                    _ => channel.max_family_clients.to_string(),
                }},
                {"name": "Needed Talk Power", "value": channel.needed_talk_power},
            ],
            "augmentation": {
                "first_prefix": augmentation.prefix.first,
                "middle_prefix": augmentation.prefix.middle,
                "last_prefix": augmentation.prefix.last,
            },
            "name": augmentation.identifier,
        }),
    ))
}

#[get("/client/<id>")]
pub async fn client(
    client: &State<Arc<AugmentationClient>>,
    id: i32,
) -> Result<Template, Redirect> {
    let config = client.config.lock().await;
    let tree = match build_tree(&*client.client.read().await, &config.internal.augmentations).await {
        Ok(tree) => tree,
        Err(e) => {
            // Connection down — send the user to the index, which shows the banner.
            error!("Could not build tree: {e}");
            return Err(Redirect::to("/"));
        }
    };
    drop(config);
    let client = match client.client.read().await.client_info(id).await {
        Ok(client) => client,
        Err(_) => {
            return Err(Redirect::to("/"));
        }
    };

    #[derive(Default, Deserialize)]
    struct ClientMetaData {
        _myts_token: Option<String>,
        tag: Option<String>,
        _updated: Option<u64>,
    }

    let meta_data = client.meta_data.clone();
    // parse json meta data
    let meta_data = match meta_data {
        Some(meta_data) => match serde_json::from_str::<ClientMetaData>(&meta_data) {
            Ok(meta_data) => meta_data,
            Err(e) => {
                error!("Could not parse meta data: {e}");
                ClientMetaData::default()
            }
        },
        None => ClientMetaData::default(),
    };

    Ok(Template::render(
        "client",
        json!({
            "connected": true,
            "tree": tree,
            "properties": [
                {"name": "Phonetic Name", "value": client.nickname_phonetic},
                {"name": "Description", "value": client.description},
                {"name": "MyTS ID", "value": meta_data.tag},
                {"name": "Total Connections", "value": client.total_connections},
                {"name": "DB ID", "value": client.database_id},
                {"name": "Version", "value": client.version},
                {"name": "Platform", "value": client.platform},
                {"name": "Talk Power", "value": client.talk_power},
                {"name": "IP", "value": client.client_ip},
            ],
            "name": client.nickname,
        }),
    ))
}

// =============
// API endpoints
// =============

#[post("/augmentation/<name>/augment", format = "json", data = "<prefix>")]
pub async fn augment(
    client: &State<Arc<AugmentationClient>>,
    name: &str,
    prefix: Json<AugmentationPrefix>,
) -> String {
    let name = match String::from_utf8(
        match general_purpose::URL_SAFE_NO_PAD.decode(name.as_bytes()) {
            Ok(name) => name,
            Err(e) => {
                error!("Could not decode augmentation name: {e}");
                return e.to_string();
            }
        },
    ) {
        Ok(name) => name,
        Err(e) => {
            error!("Could not decode augmentation name: {e}");
            return e.to_string();
        }
    };

    if let Err(e) = client.add_augmentation(&name, prefix.into_inner()).await {
        error!("Could not augment channel: {e}");
        return e.to_string();
    }

    "Success".to_string()
}

#[post("/augmentation/<name>/abridge")]
pub async fn abridge(client: &State<Arc<AugmentationClient>>, name: &str) -> String {
    let name = match String::from_utf8(
        match general_purpose::URL_SAFE_NO_PAD.decode(name.as_bytes()) {
            Ok(name) => name,
            Err(e) => {
                error!("Could not decode augmentation name: {e}");
                return e.to_string();
            }
        },
    ) {
        Ok(name) => name,
        Err(e) => {
            error!("Could not decode augmentation name: {e}");
            return e.to_string();
        }
    };

    if let Err(e) = client.remove_augmentation(&name).await {
        error!("Could not remove augmentation: {e}");
        return e.to_string();
    }

    "Success".to_string()
}

#[post(
    "/augmentation/<name>/change_prefix",
    format = "json",
    data = "<prefix>"
)]
pub async fn change_prefix(
    client: &State<Arc<AugmentationClient>>,
    name: &str,
    prefix: Json<AugmentationPrefix>,
) -> String {
    let name = match String::from_utf8(
        match general_purpose::URL_SAFE_NO_PAD.decode(name.as_bytes()) {
            Ok(name) => name,
            Err(e) => {
                error!("Could not decode augmentation name: {e}");
                return e.to_string();
            }
        },
    ) {
        Ok(name) => name,
        Err(e) => {
            error!("Could not decode augmentation name: {e}");
            return e.to_string();
        }
    };

    if let Err(e) = client
        .change_augmentation_prefix(&name, prefix.into_inner())
        .await
    {
        error!("Could not change augmentation prefix: {e}");
        return e.to_string();
    }

    "Success".to_string()
}
