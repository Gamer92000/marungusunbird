use log::warn;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::vec;
use strsim::jaro;
use thiserror::Error;
use tokio::sync::Mutex;
use ts3_query_api::definitions::Permission;
use ts3_query_api::definitions::{ChannelListEntry, ChannelProperty, EventType};
use ts3_query_api::error::QueryError;
use ts3_query_api::QueryClient;

use crate::config::{Config, ConfigError};

#[derive(Error, Debug)]
pub enum AugmentationError {
    #[error("Augmentation not found")]
    NotFound,
    #[error("Query Error: {0}")]
    Query(#[from] QueryError),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AugmentationPrefix {
    first: String,
    middle: String,
    last: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Augmentation {
    pub identifier: String,
    pub parent: i32,
    pub prefix: AugmentationPrefix,
    pub permissions: Vec<Permission>,
    pub properties: Vec<ChannelProperty>,
}

impl Augmentation {
    pub fn is_instance(&self, channel_name: &str) -> bool {
        let filter = Regex::new(&format!(
            r"^(({pre1})|({pre2})|({pre3})){} [IVXLCDM]+$",
            regex::escape(&self.identifier),
            pre1 = regex::escape(&self.prefix.first),
            pre2 = regex::escape(&self.prefix.middle),
            pre3 = regex::escape(&self.prefix.last)
        ))
        .unwrap();
        filter.is_match(channel_name)
    }
}

pub struct AugmentationClient {
    pub client: QueryClient,
    pub config: Mutex<Config>,
}

impl AugmentationClient {
    pub async fn new() -> Result<Self, ConfigError> {
        let config = Config::read_config()?;

        let client = QueryClient::connect((config.external.host.clone(), config.external.port))
            .await
            .unwrap();

        client
            .login(&config.external.user, &config.external.pass)
            .await
            .unwrap();

        client.use_sid(config.external.vsid).await.unwrap();

        client
            .server_notify_register(EventType::Channel)
            .await
            .unwrap();

        let ret = Self {
            client,
            config: Mutex::new(config),
        };

        for augmentation in ret.config.lock().await.internal.augmentations.iter() {
            ret.recover_augmentation(augmentation).await.unwrap();
        }

        // find potential augmentations managed by another instance
        let channels = ret.client.channel_list().await.unwrap();
        let pot_augmentation_regex = Regex::new(r"^.*[IVXLCDM]+$").unwrap();

        // group channel by string similarity
        let mut groups: Vec<Vec<&ChannelListEntry>> = vec![];
        for channel in channels.iter() {
            if pot_augmentation_regex.is_match(&channel.name) {
                let mut found = false;
                for group in groups.iter_mut() {
                    if jaro(&group[group.len() - 1].name, &channel.name) > 0.8 {
                        group.push(channel);
                        found = true;
                        break;
                    }
                }
                if !found {
                    groups.push(vec![channel]);
                }
            }
        }

        let config = ret.config.lock().await;
        // check if there are any groups with more than one channel that are not augmented by this instance
        let groups = groups
            .into_iter()
            .filter(|g| g.len() > 1)
            .filter(|g| {
                !config
                    .internal
                    .augmentations
                    .iter()
                    .any(|a| a.is_instance(&g[0].name))
            })
            .collect::<Vec<_>>();

        groups.iter().for_each(|g| {
            warn!(
                "Found potential augmentations: {}",
                g.iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        });
        if !groups.is_empty() {
            warn!("Please ensure that Marungu Sunbird is only run once per virtual server. If there are any additional instances, please remove them. If you believe this warning is a false positive, you may safely ignore this warning.");
        }

        drop(config);

        Ok(ret)
    }

    async fn create_channel(
        &self,
        name: &str,
        properties: &[ChannelProperty],
        permissions: &Vec<Permission>,
    ) -> Result<i32, QueryError> {
        let channel = self.client.channel_create(name, properties).await?;
        println!("Created channel {}", channel);
        if !permissions.is_empty() {
            self.client
                .channel_add_perm_multiple(channel, permissions)
                .await?;
        }
        Ok(channel)
    }

    async fn change_properties(
        &self,
        channel: &ChannelListEntry,
        mut properties: Vec<ChannelProperty>,
    ) -> Result<(), QueryError> {
        // remove properties that are already set
        properties.retain(|p| {
            if let ChannelProperty::Name(name) = p {
                *name != channel.name
            } else if let ChannelProperty::Order(order) = p {
                *order != channel.order
            } else {
                true
            }
        });
        if !properties.is_empty() {
            self.client.channel_edit(channel.id, &properties).await?;
        }
        Ok(())
    }

    fn get_augmentation_instances<'a>(
        &'a self,
        augmentation: &Augmentation,
        channels: &'a [ChannelListEntry],
    ) -> Vec<&ChannelListEntry> {
        channels
            .iter()
            .filter(|c| augmentation.is_instance(&c.name))
            .collect::<Vec<_>>()
    }

    fn get_empty_instances<'a>(
        &'a self,
        channels: &'a [&ChannelListEntry],
    ) -> Vec<&&ChannelListEntry> {
        channels
            .iter()
            .filter(|c: &&&ts3_query_api::definitions::ChannelListEntry| c.total_clients == 0)
            .collect::<Vec<_>>()
    }

    pub async fn update_augmented_channels(&self) -> Result<(), QueryError> {
        let channels = self.client.channel_list().await?;
        for augmentation in self.config.lock().await.internal.augmentations.iter() {
            // ensure there is always exactly one empty channel with the name
            // "<channel> <n>", where <n> is a roman numeral

            // find all channels with the name and any number of the augmented channel
            let augmentation_instances = self.get_augmentation_instances(augmentation, &channels);

            // if there is no channel, ignore it with warning
            if augmentation_instances.is_empty() {
                warn!("Channel {} not found", augmentation.identifier);
                continue;
            }

            // count empty channels
            let empty_channels = self.get_empty_instances(&augmentation_instances);

            // everything is fine if there is either
            //  * exactly one empty channel and it is the last one or
            //  * only two empty channels
            if empty_channels.len() == 1
                && empty_channels[0].id == augmentation_instances.last().unwrap().id
                || empty_channels.len() == 2 && augmentation_instances.len() == 2
            {
                continue;
            }

            if empty_channels.len() > 2 {
                // this should never happen, warn and ignore
                warn!(
                    "More than 2 empty channels found for augmentation {}",
                    augmentation.identifier
                );
            }

            if empty_channels.len() > 1 {
                // cleanup required, move and rename channels
                let target_order = empty_channels[0].order;
                let target_name = empty_channels[0].name.clone();
                // if empty channel is the second to last overall, simply remove the last one
                if empty_channels[0].id
                    == augmentation_instances[augmentation_instances.len() - 2].id
                {
                    self.client
                        .channel_delete(empty_channels[empty_channels.len() - 1].id, false)
                        .await?;
                    // rename empty channel to have the last prefix
                    self.change_properties(
                        empty_channels[0],
                        vec![ChannelProperty::Name(format!(
                            "{}{} {}",
                            augmentation.prefix.last,
                            augmentation.identifier,
                            roman::to(augmentation_instances.len() as i32 - 1).unwrap()
                        ))],
                    )
                    .await?;
                } else {
                    // delete the empty channel
                    self.client
                        .channel_delete(empty_channels[0].id, false)
                        .await?;
                    // move the pre to last (presumably non empty) channel to the empty channel
                    let mut replacement_name = augmentation_instances
                        [augmentation_instances.len() - 2]
                        .name
                        .clone();
                    replacement_name.replace_range(
                        0..augmentation.prefix.middle.len(),
                        &augmentation.prefix.last,
                    );
                    self.client
                        .channel_edit(
                            augmentation_instances[augmentation_instances.len() - 2].id,
                            &[
                                ChannelProperty::Order(target_order),
                                ChannelProperty::Name(target_name),
                            ],
                        )
                        .await?;
                    // rename the last (presumably empty) channel
                    self.client
                        .channel_edit(
                            augmentation_instances[augmentation_instances.len() - 1].id,
                            &[ChannelProperty::Name(replacement_name)],
                        )
                        .await?;
                }
            } else if empty_channels.is_empty() {
                // simply add a new empty channel
                let mut props = augmentation.properties.clone();
                props.push(ChannelProperty::Order(
                    augmentation_instances.last().unwrap().id,
                ));
                self.create_channel(
                    &format!(
                        "{}{} {}",
                        augmentation.prefix.last,
                        augmentation.identifier,
                        roman::to(augmentation_instances.len() as i32 + 1).unwrap()
                    ),
                    &props,
                    augmentation.permissions.as_ref(),
                )
                .await?;
                // rename the last channel to have the middle prefix
                self.change_properties(
                    augmentation_instances.last().unwrap(),
                    vec![ChannelProperty::Name(format!(
                        "{}{} {}",
                        augmentation.prefix.middle,
                        augmentation.identifier,
                        roman::to(augmentation_instances.len() as i32).unwrap()
                    ))],
                )
                .await?;
            } else {
                // move all clients from the last channel to the empty channel
                // get all clients from the last channel
                let clients = self.client.client_list().await?;
                let clients = clients
                    .into_iter()
                    .filter(|c| c.channel_id == augmentation_instances.last().unwrap().id)
                    .map(|c| c.id)
                    .collect::<Vec<_>>();
                // move all clients to the empty channel
                self.client
                    .client_move(&clients, empty_channels[0].id, None, true)
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn recover_augmentation(
        &self,
        augmentation: &Augmentation,
    ) -> Result<(), AugmentationError> {
        let channels = self.client.channel_list().await?;

        for channel in channels.iter() {
            println!("{}", channel.name);
        }

        let augmentation_instances = self.get_augmentation_instances(augmentation, &channels);

        // if there is no channel, create one
        if augmentation_instances.is_empty() {
            self.create_channel(
                &format!("{}{} I", augmentation.prefix.first, augmentation.identifier),
                &augmentation.properties,
                &augmentation.permissions,
            )
            .await?;
            self.create_channel(
                &format!("{}{} II", augmentation.prefix.last, augmentation.identifier),
                &augmentation.properties,
                &augmentation.permissions,
            )
            .await?;
            return Ok(());
        }

        let empty_channels = self.get_empty_instances(&augmentation_instances);

        let mut offset = 2;
        if augmentation_instances.len() > empty_channels.len() {
            offset = 1;
        }
        // delete all but the last empty channel
        let mut remaining_channels = augmentation_instances.clone();
        if empty_channels.len() > offset {
            for channel in empty_channels[..empty_channels.len() - offset].iter() {
                self.client.channel_delete(channel.id, false).await?;
                remaining_channels.retain(|c| c.id != channel.id);
            }
        }

        // rename all other channels to the correct roman numeral
        for (i, channel) in remaining_channels.iter().enumerate() {
            let mut prefix = augmentation.prefix.middle.as_str();
            if i == 0 {
                prefix = augmentation.prefix.first.as_str();
            } else if i == remaining_channels.len() - 1 {
                prefix = augmentation.prefix.last.as_str();
            }
            let name = format!(
                "{}{} {}",
                prefix,
                augmentation.identifier,
                roman::to(i as i32 + 1).unwrap()
            );
            self.change_properties(channel, vec![ChannelProperty::Name(name)])
                .await?;
        }
        // if the remaining empty channel is not the last one, move all clients from the last one to the empty one
        if !empty_channels.is_empty() {
            if empty_channels[empty_channels.len() - 1].id
                != augmentation_instances.last().unwrap().id
            {
                // get all clients from the last channel
                let clients = self.client.client_list().await?;
                let clients = clients
                    .into_iter()
                    .filter(|c| c.id == augmentation_instances.last().unwrap().id)
                    .map(|c| c.id)
                    .collect::<Vec<_>>();
                // move all clients to the empty channel
                self.client
                    .client_move(
                        &clients,
                        empty_channels[empty_channels.len() - 1].id,
                        None,
                        true,
                    )
                    .await?;
            }
        } else {
            // create a new empty channel
            let mut props = augmentation.properties.clone();
            props.push(ChannelProperty::Order(
                augmentation_instances.last().unwrap().id,
            ));

            self.create_channel(
                &format!(
                    "{}{} {}",
                    augmentation.prefix.last,
                    augmentation.identifier,
                    roman::to(augmentation_instances.len() as i32 + 1).unwrap()
                ),
                &props,
                &augmentation.permissions,
            )
            .await?;
            // rename the last channel to have the middle prefix
            self.change_properties(
                augmentation_instances.last().unwrap(),
                vec![ChannelProperty::Name(format!(
                    "{}{} {}",
                    augmentation.prefix.middle,
                    augmentation.identifier,
                    roman::to(augmentation_instances.len() as i32).unwrap()
                ))],
            )
            .await?;
        }

        Ok(())
    }

    pub async fn add_augmentation(
        &self,
        identifier: &str,
        prefix: AugmentationPrefix,
    ) -> Result<(), AugmentationError> {
        // ensure there are no overlaps in the augmented channels
        if self
            .config
            .lock()
            .await
            .internal
            .augmentations
            .iter()
            .any(|c| c.identifier == identifier)
        {
            return Err(AugmentationError::NotFound);
        }

        let channels = self.client.channel_list().await?;
        // find a channel with the name <identifier>
        let channel = channels
            .iter()
            .find(|c| c.name == identifier)
            .ok_or(AugmentationError::NotFound)?;

        // find all permissions of the channel
        let permissions = self.client.channel_perm_list(channel.id).await?;
        let permissions = permissions.into_iter().map(|p| p.perm).collect::<Vec<_>>();

        // find all channel properties
        let info = self.client.channel_info(channel.id).await?;
        let mut properties = info.to_properties_vec();
        properties.retain(|p| {
            !matches!(
                p,
                ChannelProperty::Name(_)
                    | ChannelProperty::Order(_)
                    | ChannelProperty::FlagDefault(_)
            )
        });

        self.client
            .channel_edit(
                channel.id,
                &[ChannelProperty::Name(format!(
                    "{}{} I",
                    prefix.first, identifier,
                ))],
            )
            .await?;

        let mut props = properties.clone();
        props.push(ChannelProperty::Order(channel.id));

        self.create_channel(
            &format!("{}{} II", prefix.last, identifier,),
            &props,
            &permissions,
        )
        .await?;

        let augmentation = Augmentation {
            identifier: identifier.to_string(),
            parent: channel.parent_id,
            prefix: prefix.clone(),
            permissions,
            properties,
        };

        self.config
            .lock()
            .await
            .add_augmentation(augmentation.clone());

        Ok(())
    }

    pub async fn remove_augmentation(&self, identifier: &str) -> Result<(), AugmentationError> {
        let mut augmentation = self.config.lock().await;
        let augmentation = augmentation.remove_augmentation(identifier)?;
        let channels = self.client.channel_list().await?;
        let augmentation_instances = self.get_augmentation_instances(&augmentation, &channels);
        // move all users to the first channel
        let clients = self.client.client_list().await?;
        let clients = clients
            .into_iter()
            .filter(|c| {
                augmentation_instances[1..]
                    .iter()
                    .any(|a| a.id == c.channel_id)
            })
            .map(|c| c.id)
            .collect::<Vec<_>>();
        if !clients.is_empty() {
            self.client
                .client_move(&clients, augmentation_instances[0].id, None, true)
                .await?;
        }
        // delete all other channels
        for channel in augmentation_instances[1..].iter() {
            self.client.channel_delete(channel.id, false).await?;
        }
        // rename the first channel
        self.change_properties(
            augmentation_instances[0],
            vec![ChannelProperty::Name(identifier.to_string())],
        )
        .await?;

        Ok(())
    }

    pub async fn get_augmentation_of_channel(
        &self,
        channel: &str,
    ) -> Result<String, AugmentationError> {
        let augmentations = self.config.lock().await.internal.augmentations.clone();
        let augmentation = augmentations
            .iter()
            .find(|a| a.is_instance(channel))
            .ok_or(AugmentationError::NotFound)?;
        Ok(augmentation.identifier.clone())
    }
}
