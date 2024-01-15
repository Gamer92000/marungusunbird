use base64::{engine::general_purpose, Engine as _};
use chrono::Duration;
use lazy_static::lazy_static;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::{cell::Cell, collections::HashMap, fs};
use ts3_query_api::definitions::{ChannelListDynamicEntry, ClientListDynamicEntry};

use crate::{badges::BadgesFile, errors::Error};

#[derive(Serialize, Clone)]
pub struct Channel {
    pub id: i32,
    pub name: String,
    pub parent_id: i32,
    pub talk_power: i32,
    pub is_augmented: bool,
    pub augmentation_id: Option<String>,
    pub highlight_color: Option<String>,
    pub indent_level: Cell<i32>,
}

#[derive(Serialize, Clone)]
pub struct Client {
    pub id: i32,
    pub name: String,
    pub channel: i32,
    pub is_query: bool,
    pub talk_power: i32,
    pub can_talk: bool,
    pub badges: Vec<String>,
    pub country: Option<String>,
}

impl From<ChannelListDynamicEntry> for Channel {
    fn from(channel: ChannelListDynamicEntry) -> Self {
        Self {
            id: channel.base.id,
            name: channel.base.name,
            parent_id: channel.base.parent_id,
            talk_power: channel.voice.map_or(0, |v| v.needed_talk_power),
            is_augmented: false,
            augmentation_id: None,
            highlight_color: None,
            indent_level: Cell::new(0),
        }
    }
}

impl From<ClientListDynamicEntry> for Client {
    fn from(client: ClientListDynamicEntry) -> Self {
        Self {
            id: client.base.id,
            name: client.base.nickname,
            channel: client.base.channel_id,
            is_query: client.base.is_query,
            talk_power: client.voice.as_ref().map_or(0, |v| v.talk_power),
            can_talk: client.voice.as_ref().map_or(false, |v| v.is_talker),
            badges: client.badges.map_or(vec![], |b| b.badges.badges),
            country: client.country.and_then(|c| {
                c.country.map(|c| {
                    c.to_uppercase()
                        .chars()
                        .map(|c| match char::from_u32(0x1f1a5 + c as u32) {
                            Some(c) => c,
                            None => c,
                        })
                        .collect()
                })
            }),
        }
    }
}

pub fn extract_spacer_name(
    value: &Value,
    _args: &HashMap<String, Value>,
) -> Result<Value, rocket_dyn_templates::tera::Error> {
    lazy_static! {
        static ref SPACER_PREFIX: Regex = Regex::new(r"^\[c?spacer\]\s*").unwrap();
    }

    if let Value::String(value) = value {
        return Ok(SPACER_PREFIX.replace(value, "").into());
    }

    Ok(value.clone())
}

pub fn base64_encode(
    value: &Value,
    _args: &HashMap<String, Value>,
) -> Result<Value, rocket_dyn_templates::tera::Error> {
    if let Value::String(value) = value {
        return Ok(general_purpose::URL_SAFE_NO_PAD.encode(value).into());
    }

    Ok(value.clone())
}

pub async fn init_badges() -> Result<(), Error> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()?;

    let data = client
        .get("https://badges-content.teamspeak.com/list")
        .send()
        .await?
        .bytes()
        .await?;

    let badges = BadgesFile::parse(&data)?.badges;

    for badge in badges {
        // pull each badge svg and store it in the assets folder
        let data = client
            .get(&format!("{}.svg", badge.icon_url))
            .send()
            .await?
            .bytes()
            .await?;

        fs::write(format!("static/badges/{}.svg", badge.uuid), data)?;
    }

    Ok(())
}

pub fn format_duration(seconds: i64) -> String {
    let duration = Duration::seconds(seconds);
    let years = duration.num_weeks() / 52;
    let weeks = duration.num_weeks() % 52;
    let days = duration.num_days() % 7;
    let hours = (duration.num_hours() + 1) % 24; // round up to the next hour

    let mut result = String::new();
    if years > 0 {
        result.push_str(&format!("{} y, ", years));
    }
    if weeks > 0 {
        result.push_str(&format!("{} w, ", weeks));
    }
    if days > 0 {
        result.push_str(&format!("{} d, ", days));
    }
    result.push_str(&format!("{} h", hours));

    result
}
