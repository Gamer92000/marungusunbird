use serde::Serialize;
use std::{cell::Cell, fs};
use ts3_query_api::definitions::{ChannelListDynamicEntry, ClientListDynamicEntry};

use crate::badges::BadgesFile;

#[derive(Serialize, Clone)]
pub struct Channel {
    pub id: i32,
    pub name: String,
    pub parent_id: i32,
    pub talk_power: i32,
    pub is_augmented: bool,
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
                        .map(|c| char::from_u32(0x1f1a5 + c as u32).unwrap())
                        .collect()
                })
            }),
        }
    }
}

pub async fn init_badges() {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()
        .unwrap();

    let data = client
        .get("https://badges-content.teamspeak.com/list")
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let badges = BadgesFile::parse(&data).unwrap().badges;

    for badge in badges {
        // pull each badge svg and store it in the assets folder
        let data = client
            .get(&format!("{}.svg", badge.icon_url))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();

        fs::write(format!("static/badges/{}.svg", badge.uuid), data).unwrap();
    }
}
