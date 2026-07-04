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
    /// Spacer metadata, present only for channels that are rendered as spacers
    /// (top-level, permanent, name matching the TeamSpeak spacer pattern).
    pub spacer: Option<Spacer>,
}

/// A parsed TeamSpeak channel spacer. Mirrors the classification the official
/// TS6 client performs on the channel name (`templates/tree.html.tera` renders
/// each variant). A spacer is a top-level, permanent channel whose name matches
/// `^\[<align>spacer<id>]<content>`.
#[derive(Serialize, Clone)]
pub struct Spacer {
    /// Horizontal alignment / fill mode: `"left"`, `"center"`, `"right"` or
    /// `"repeat"`. Derived from the character(s) before `spacer` in the tag
    /// (`c`/`r`/`l`/`*`); anything else defaults to `"left"`.
    pub align: &'static str,
    /// `true` for `[*spacer…]`: the content is repeated to fill the row width.
    pub repeat: bool,
    /// For non-repeat spacers whose content is exactly one of the reserved line
    /// patterns, the stroke style to draw: `"solid"` (`___`), `"dash"` (`---`),
    /// `"dot"` (`...`), `"dash-dot"` (`-.-`) or `"dash-dot-dot"` (`-..`). Empty
    /// otherwise.
    pub line: &'static str,
    /// The display text: the channel name with the leading `[…spacer…]` tag
    /// stripped. For repeat spacers this is the unit that gets tiled.
    pub text: String,
}

impl Spacer {
    /// Classify `name` as a spacer, or return `None` if it is an ordinary
    /// channel. `top_level` and `permanent` are the two structural conditions
    /// TeamSpeak also requires — only top-level permanent channels can be
    /// spacers, everything else keeps its literal name.
    pub fn parse(name: &str, top_level: bool, permanent: bool) -> Option<Spacer> {
        lazy_static! {
            // Group 1: the tag text before "spacer" (alignment marker).
            // Group 2: everything after the closing bracket (the content).
            static ref SPACER_RE: Regex = Regex::new(r"^\[([^\]]*)spacer[^\]]*\](.*)$").unwrap();
        }
        if !top_level || !permanent {
            return None;
        }
        let caps = SPACER_RE.captures(name)?;
        let marker = caps.get(1).map_or("", |m| m.as_str());
        let content = caps.get(2).map_or("", |m| m.as_str());

        let repeat = name.starts_with("[*");
        let align = if repeat {
            "repeat"
        } else {
            match marker {
                "c" => "center",
                "r" => "right",
                "l" => "left",
                _ => "left",
            }
        };
        // A repeat spacer tiles its (usually single-character) content, so the
        // reserved multi-character line patterns never apply to it.
        let line = if repeat {
            ""
        } else {
            match content {
                "___" => "solid",
                "---" => "dash",
                "..." => "dot",
                "-.-" => "dash-dot",
                "-.." => "dash-dot-dot",
                _ => "",
            }
        };

        Some(Spacer {
            align,
            repeat,
            line,
            text: content.to_string(),
        })
    }
}

#[derive(Serialize, Clone)]
pub struct Client {
    pub id: i32,
    pub name: String,
    pub channel: i32,
    pub is_query: bool,
    pub talk_power: i32,
    pub can_talk: bool,
    /// Voice status shown in the tree, highest priority first:
    /// "afk" > "sound_muted" > "mic_muted" > "mic_disabled" > "normal".
    pub state: &'static str,
    pub badges: Vec<String>,
    pub country: Option<String>,
}

impl From<ChannelListDynamicEntry> for Channel {
    fn from(channel: ChannelListDynamicEntry) -> Self {
        let permanent = channel.flags.as_ref().is_some_and(|f| f.flag_permanent);
        let spacer = Spacer::parse(&channel.base.name, channel.base.parent_id == 0, permanent);
        Self {
            id: channel.base.id,
            name: channel.base.name,
            parent_id: channel.base.parent_id,
            talk_power: channel.voice.map_or(0, |v| v.needed_talk_power),
            is_augmented: false,
            augmentation_id: None,
            highlight_color: None,
            indent_level: Cell::new(0),
            spacer,
        }
    }
}

impl From<ClientListDynamicEntry> for Client {
    fn from(client: ClientListDynamicEntry) -> Self {
        let away = client.away.as_ref().is_some_and(|a| a.away);
        let (input_muted, output_muted, input_hardware, output_hardware) = client
            .voice
            .as_ref()
            .map_or((false, false, true, true), |v| {
                (v.input_muted, v.output_muted, v.input_hardware, v.output_hardware)
            });
        // Priority: afk > sound disabled > sound mute > mic disabled > mic mute.
        let state = if away {
            "afk"
        } else if !output_hardware {
            "sound_disabled"
        } else if output_muted {
            "sound_muted"
        } else if !input_hardware {
            "mic_disabled"
        } else if input_muted {
            "mic_muted"
        } else {
            "normal"
        };

        Self {
            id: client.base.id,
            name: client.base.nickname,
            channel: client.base.channel_id,
            is_query: client.base.is_query,
            talk_power: client.voice.as_ref().map_or(0, |v| v.talk_power),
            can_talk: client.voice.as_ref().is_some_and(|v| v.is_talker),
            state,
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
            .get(format!("{}.svg", badge.icon_url))
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
        result.push_str(&format!("{years} y, "));
    }
    if weeks > 0 {
        result.push_str(&format!("{weeks} w, "));
    }
    if days > 0 {
        result.push_str(&format!("{days} d, "));
    }
    result.push_str(&format!("{hours} h"));

    result
}

#[cfg(test)]
mod tests {
    use super::Spacer;

    fn parse(name: &str) -> Option<Spacer> {
        // Every spacer on a real server is a top-level permanent channel.
        Spacer::parse(name, true, true)
    }

    #[test]
    fn non_spacer_channels_are_ignored() {
        assert!(parse("General").is_none());
        assert!(parse("╓─ Home Sweet Home I").is_none());
        // The keyword alone is not a spacer tag.
        assert!(parse("spacer stuff").is_none());
    }

    #[test]
    fn requires_top_level_and_permanent() {
        assert!(Spacer::parse("[cspacer]Internal", false, true).is_none());
        assert!(Spacer::parse("[cspacer]Internal", true, false).is_none());
    }

    #[test]
    fn alignment_markers() {
        assert_eq!(parse("[spacer]hi").unwrap().align, "left");
        assert_eq!(parse("[lspacer]hi").unwrap().align, "left");
        assert_eq!(parse("[cspacer]Internal").unwrap().align, "center");
        assert_eq!(parse("[rspacer]wtf").unwrap().align, "right");
        assert_eq!(parse("[*spacer]-").unwrap().align, "repeat");
    }

    #[test]
    fn text_is_stripped_of_tag() {
        assert_eq!(parse("[cspacer]Welcome").unwrap().text, "Welcome");
        assert_eq!(parse("[rspacer]wtf").unwrap().text, "wtf");
        // Unique identifiers inside the tag are allowed and stripped.
        assert_eq!(parse("[cspacer42] External").unwrap().text, " External");
    }

    #[test]
    fn repeat_spacers() {
        let s = parse("[*spacer]-").unwrap();
        assert!(s.repeat);
        assert_eq!(s.text, "-");
        assert_eq!(s.line, "", "repeat spacers never draw a css line");
        let u = parse("[*spacer]_").unwrap();
        assert!(u.repeat);
        assert_eq!(u.text, "_");
    }

    #[test]
    fn line_spacers() {
        assert_eq!(parse("[spacer]___").unwrap().line, "solid");
        assert_eq!(parse("[spacer]---").unwrap().line, "dash");
        assert_eq!(parse("[spacer]...").unwrap().line, "dot");
        assert_eq!(parse("[spacer]-.-").unwrap().line, "dash-dot");
        assert_eq!(parse("[spacer]-..").unwrap().line, "dash-dot-dot");
        // Non-repeat line spacers keep their raw content as (unused) text.
        assert!(!parse("[spacer]---").unwrap().repeat);
    }

    #[test]
    fn plain_text_spacer_has_no_line() {
        let s = parse("[cspacer]Internal").unwrap();
        assert_eq!(s.line, "");
        assert!(!s.repeat);
    }
}
