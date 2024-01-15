use log::error;
use rand::Rng;
use rand_pcg::Pcg64;
use rand_seeder::Seeder;
use serde::Serialize;
use std::collections::HashMap;
use ts3_query_api::{
    definitions::builder::{ChannelListFlags, ClientListFlags},
    QueryClient,
};

use crate::{
    augmentation::Augmentation,
    errors::Error,
    helper::{Channel, Client},
};

#[derive(Serialize)]
pub struct Tree {
    pub server_name: String,
    pub channel_order: Vec<i32>,
    pub channel_map: HashMap<i32, Channel>,
    pub clients: HashMap<i32, Vec<Client>>,
}

pub async fn build_tree(
    client: &QueryClient,
    augmentations: &[Augmentation],
) -> Result<Tree, Error> {
    let server = client.server_info().await?;
    let server_name = server.name;

    let channels = client
        .channel_list_dynamic(ChannelListFlags::default().with_voice().with_flags())
        .await?;

    let channels = channels.into_iter().map(Channel::from).collect::<Vec<_>>();

    let channel_order = channels.iter().map(|c| c.id).collect::<Vec<_>>();

    let mut channel_map = channels
        .iter()
        .map(|c| (c.id, c.clone()))
        .collect::<HashMap<_, _>>();

    channel_map.values().for_each(|c| {
        if c.parent_id != 0 {
            c.indent_level.set(
                match channel_map.get(&c.parent_id) {
                    Some(channel) => channel.indent_level.get(),
                    None => {
                        error!("Could not find parent channel with id {}", c.parent_id);
                        0
                    }
                } + 1,
            );
        }
    });

    let clients = client
        .client_list_dynamic(
            ClientListFlags::default()
                .with_voice()
                .with_badges()
                .with_groups()
                .with_country(),
        )
        .await?;
    let clients = clients.into_iter().map(Client::from).collect::<Vec<_>>();
    // group clients by channel
    let mut clients_by_channel = HashMap::new();
    for client in clients {
        let channel = client.channel;
        clients_by_channel
            .entry(channel)
            .or_insert_with(Vec::new)
            .push(client);
    }

    // calculate whether a client can talk
    // (check if client talk power is higher than channel talk power)
    for (channel, clients) in &mut clients_by_channel {
        let channel = match channel_map.get(channel) {
            Some(channel) => channel,
            None => {
                error!("Could not find channel with id {}", channel);
                continue;
            }
        };
        for client in clients {
            if client.talk_power >= channel.talk_power {
                client.can_talk = true;
            }
        }
    }

    // sort clients by talk power and name
    for clients in clients_by_channel.values_mut() {
        clients.sort_by(|c1, c2| {
            // first sort by talk power
            let talk_power1 = c1.talk_power;
            let talk_power2 = c2.talk_power;
            talk_power2
                .cmp(&talk_power1)
                .then_with(|| c1.name.cmp(&c2.name))
        });
    }

    // add augmentation functionality to the treeitems
    for augmented_channel in augmentations.iter() {
        let mut rng: Pcg64 = Seeder::from(augmented_channel.identifier.as_bytes()).make_rng();
        // generate a random color for each augmentation group
        let color = format!(
            "hsl({}, {}%, {}%)",
            rng.gen_range(0.0..360.0),
            rng.gen_range(40.0..90.0),
            rng.gen_range(60.0..80.0)
        );

        for channel in channel_map.values_mut() {
            if augmented_channel.is_instance(&channel.name) {
                channel.is_augmented = true;
                channel.augmentation_id = Some(augmented_channel.identifier.clone());
                channel.highlight_color = color.clone().into();
            }
        }
    }

    Ok(Tree {
        server_name,
        channel_order,
        channel_map,
        clients: clients_by_channel,
    })
}
