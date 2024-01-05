use thiserror::Error;
use ts3_query_api::error::QueryError;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Augmentation not found")]
    NotFound,
    #[error("Regex Error: {0}")]
    Regex(#[from] regex::Error),
    #[error("Query Error: {0}")]
    Query(#[from] QueryError),
    #[error("Could not parse state: {0}")]
    State1(#[from] ron::Error),
    #[error("Could not parse state: {0}")]
    State2(#[from] ron::de::SpannedError),
    #[error("Could not parse config: {0}")]
    Parse(#[from] rocket::figment::Error),
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Badge parse error: {0}")]
    BadgeParse(#[from] crate::badges::ParseError),
}
