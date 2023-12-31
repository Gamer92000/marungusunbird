use serde::Serialize;
use std::collections::HashMap;
use ts3_query_api::{
    definitions::builder::{ChannelListFlags, ClientListFlags},
    QueryClient,
};

use crate::helper::{Channel, Client};

#[derive(Serialize)]
pub struct Tree {
    pub channel_order: Vec<i32>,
    pub channel_map: HashMap<i32, Channel>,
    pub clients: HashMap<i32, Vec<Client>>,
}

pub async fn build_tree(client: &QueryClient) -> Tree {
    let channels = client
        .channel_list_dynamic(ChannelListFlags::default().with_voice().with_flags())
        .await
        .unwrap_or_default();

    let channels = channels.into_iter().map(Channel::from).collect::<Vec<_>>();

    let channel_order = channels.iter().map(|c| c.id).collect::<Vec<_>>();

    let channel_map = channels
        .iter()
        .map(|c| (c.id, c.clone()))
        .collect::<HashMap<_, _>>();

    channel_map.values().for_each(|c| {
        if c.parent_id != 0 {
            c.indent_level
                .set(channel_map.get(&c.parent_id).unwrap().indent_level.get() + 1);
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
        .await
        .unwrap_or_default();
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
        let channel = channel_map.get(channel).unwrap();
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

    Tree {
        channel_order,
        channel_map,
        clients: clients_by_channel,
    }
}
