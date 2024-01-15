use crate::errors::Error;
use log::{debug, info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::vec;
use strsim::jaro;
use tokio::sync::Mutex;
use ts3_query_api::definitions::Permission;
use ts3_query_api::definitions::{ChannelListEntry, ChannelProperty};
use ts3_query_api::error::QueryError;
use ts3_query_api::QueryClient;

use crate::config::Config;

#[derive(Clone, Serialize, Deserialize)]
pub struct AugmentationPrefix {
    pub first: String,
    pub middle: String,
    pub last: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Augmentation {
    pub identifier: String,
    pub parent: i32,
    pub prefix: AugmentationPrefix,
    pub permissions: Vec<Permission>,
    pub properties: Vec<ChannelProperty>,
    #[serde(skip)]
    pub regex: OnceLock<Regex>,
}

impl Augmentation {
    pub fn is_instance(&self, channel_name: &str) -> bool {
        self.regex
            .get_or_init(|| {
                Regex::new(&format!(
                    r"^(({pre1})|({pre2})|({pre3})){} [IVXLCDM]+$",
                    regex::escape(&self.identifier),
                    pre1 = regex::escape(&self.prefix.first),
                    pre2 = regex::escape(&self.prefix.middle),
                    pre3 = regex::escape(&self.prefix.last)
                ))
                .unwrap()
            })
            .is_match(channel_name)
    }

    pub fn set_prefix(&mut self, prefix: AugmentationPrefix) {
        self.prefix = prefix;
        self.regex = OnceLock::new();
    }
}

pub struct AugmentationClient {
    pub client: QueryClient,
    pub config: Mutex<Config>,
}

impl AugmentationClient {
    pub async fn new() -> Result<Self, Error> {
        let config = Config::read_config()?;

        info!(
            "Connecting to server {}:{}",
            config.external.host, config.external.port
        );

        let client =
            QueryClient::connect((config.external.host.clone(), config.external.port)).await?;

        info!("Logging in as {}", config.external.user);

        client
            .login(&config.external.user, &config.external.pass)
            .await?;

        info!("Using virtual server {}", config.external.vsid);

        client.use_sid(config.external.vsid).await?;

        // TODO(requires clientupdate): change nickname to "Marungu Sunbird"

        info!("Registering for events");

        client.server_notify_register_all().await?;

        let ret = Self {
            client,
            config: Mutex::new(config),
        };

        info!("Recovering augmentations from state.bin");

        let config = ret.config.lock().await;
        for augmentation in config.internal.augmentations.iter() {
            ret.recover_augmentation(augmentation).await?;
        }

        info!(
            "Successfully recovered {} augmentation{}",
            config.internal.augmentations.len(),
            match config.internal.augmentations.len() {
                1 => "",
                _ => "s",
            }
        );
        drop(config);

        // find potential augmentations managed by another instance
        let channels = ret.client.channel_list().await?;
        let pot_augmentation_regex = Regex::new(r"^.*[IVXLCDM]+$")?;

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

        drop(config);

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

        Ok(ret)
    }

    async fn create_channel(
        &self,
        name: &str,
        properties: &[ChannelProperty],
        permissions: &Vec<Permission>,
    ) -> Result<i32, QueryError> {
        let mut properties = properties.to_vec();
        let mut icon = None;
        if let Some(index) = properties
            .iter()
            .position(|p| matches!(p, ChannelProperty::IconId(_)))
        {
            icon = Some(properties.remove(index));
        }

        let channel = self.client.channel_create(name, &properties).await?;

        if let Some(icon) = icon {
            self.client.channel_edit(channel, &[icon]).await?;
        }

        debug!("Created channel {}", channel);

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
        properties.retain(|p| match p {
            ChannelProperty::Name(name) => *name != channel.name,
            ChannelProperty::Order(order) => *order != channel.order,
            _ => true,
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
                        match roman::to(augmentation_instances.len() as i32 + 1) {
                            Some(r) => r,
                            None => {
                                warn!(
                                    "Could not convert {} to roman numeral",
                                    augmentation_instances.len() + 1
                                );
                                (augmentation_instances.len() + 1).to_string()
                            }
                        }
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
                        match roman::to(augmentation_instances.len() as i32) {
                            Some(r) => r,
                            None => {
                                warn!(
                                    "Could not convert {} to roman numeral",
                                    augmentation_instances.len()
                                );
                                augmentation_instances.len().to_string()
                            }
                        }
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

    pub async fn recover_augmentation(&self, augmentation: &Augmentation) -> Result<(), Error> {
        let channels = self.client.channel_list().await?;

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
                match roman::to(i as i32 + 1) {
                    Some(r) => r,
                    None => {
                        warn!("Could not convert {} to roman numeral", i + 1);
                        (i + 1).to_string()
                    }
                }
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
                    match roman::to(augmentation_instances.len() as i32 + 1) {
                        Some(r) => r,
                        None => {
                            warn!(
                                "Could not convert {} to roman numeral",
                                augmentation_instances.len() + 1
                            );
                            (augmentation_instances.len() + 1).to_string()
                        }
                    }
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
                    match roman::to(augmentation_instances.len() as i32) {
                        Some(r) => r,
                        None => {
                            warn!(
                                "Could not convert {} to roman numeral",
                                augmentation_instances.len()
                            );
                            augmentation_instances.len().to_string()
                        }
                    }
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
    ) -> Result<(), Error> {
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
            return Err(Error::NotFound);
        }

        let channels = self.client.channel_list().await?;
        // find a channel with the name <identifier>
        let channel = channels
            .iter()
            .find(|c| c.name == identifier)
            .ok_or(Error::NotFound)?;

        // find all permissions of the channel
        let permissions = self.client.channel_perm_list(channel.id).await?;
        let mut permissions = permissions.into_iter().map(|p| p.perm).collect::<Vec<_>>();
        permissions.push(Permission::i_channel_needed_modify_power(100));
        permissions.push(Permission::i_channel_needed_permission_modify_power(100));

        // find all channel properties
        let info = self.client.channel_info(channel.id).await?;
        let mut properties = info.to_properties_vec();
        properties.retain(|p| {
            !matches!(
                p,
                ChannelProperty::Name(_)
                    | ChannelProperty::Order(_)
                    | ChannelProperty::FlagDefault(_)
                    | ChannelProperty::Password(_)
            )
        });

        self.change_properties(
            channel,
            vec![ChannelProperty::Name(format!(
                "{}{} I",
                prefix.first, identifier,
            ))],
        )
        .await?;
        self.client
            .channel_add_perm_multiple(
                channel.id,
                &[
                    Permission::i_channel_needed_modify_power(100),
                    Permission::i_channel_needed_permission_modify_power(100),
                ],
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
            regex: OnceLock::new(),
        };

        self.config
            .lock()
            .await
            .add_augmentation(augmentation.clone())?;

        Ok(())
    }

    pub async fn remove_augmentation(&self, identifier: &str) -> Result<(), Error> {
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
        self.client
            .channel_add_perm_multiple(
                augmentation_instances[0].id,
                &[
                    Permission::i_channel_needed_modify_power(75),
                    Permission::i_channel_needed_permission_modify_power(75),
                ],
            )
            .await?;

        Ok(())
    }

    pub async fn change_augmentation_prefix(
        &self,
        identifier: &str,
        prefix: AugmentationPrefix,
    ) -> Result<(), Error> {
        let mut config = self.config.lock().await;
        let augmentation = match config
            .internal
            .augmentations
            .iter_mut()
            .find(|a| a.identifier == identifier)
        {
            Some(a) => a,
            None => return Err(Error::NotFound),
        };
        let channels = self.client.channel_list().await?;
        let augmentation_instances = self.get_augmentation_instances(augmentation, &channels);
        // rename all channels
        for (i, channel) in augmentation_instances.iter().enumerate() {
            let mut local_prefix = prefix.middle.as_str();
            if i == 0 {
                local_prefix = prefix.first.as_str();
            } else if i == augmentation_instances.len() - 1 {
                local_prefix = prefix.last.as_str();
            }
            let name = format!(
                "{}{} {}",
                local_prefix,
                augmentation.identifier,
                match roman::to(i as i32 + 1) {
                    Some(r) => r,
                    None => {
                        warn!("Could not convert {} to roman numeral", i + 1);
                        (i + 1).to_string()
                    }
                }
            );
            self.change_properties(channel, vec![ChannelProperty::Name(name)])
                .await?;
        }
        augmentation.set_prefix(prefix);

        Ok(())
    }
}
