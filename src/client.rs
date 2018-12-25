use crate::settings::Settings;
use futures::prelude::*;
use reqwest::r#async as req;
use serde_derive::{Deserialize, Serialize};
use std::fmt;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct User {
    username: String,
    nickname: String,
}

enum_number!(SubjectType {
    Book = 1,
    Anime = 2,
    Music = 3,
    Game = 4,
    Real = 6,
});

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubjectSmall {
    id: u64,
    air_date: String,
    air_weekday: u8,

    name: String,
    name_cn: String,
    summary: String,

    #[serde(rename = "type")]
    subject_type: SubjectType,

    url: String,

    vols_count: Option<u64>,
    eps_count: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollectionEntry {
    ep_status: u64,
    vol_status: u64,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub(crate) lasttouch: chrono::DateTime<chrono::Utc>,

    subject: SubjectSmall,
}

pub struct Client {
    settings: Settings,
}

trait ClientAuthBearer {
    fn apply_auth(self, info: &Client) -> Self;
}

impl ClientAuthBearer for req::RequestBuilder {
    fn apply_auth(self, info: &Client) -> Self {
        if let Some(handle) = info.settings.auth() {
            self.header(
                "Authorization",
                format!("Bearer {}", handle.info.access_token),
            )
        } else {
            self
        }
    }
}

impl Client {
    pub fn new(settings: Settings) -> Client {
        Client { settings: settings }
    }

    pub fn user(&self, uid: Option<u64>) -> impl Future<Item = User, Error = failure::Error> {
        let c = req::Client::new();
        let uid = uid.unwrap_or(self.settings.auth().as_ref().unwrap().info.user_id);
        c.get(&format!("{}/user/{}", API_ROOT!(), uid))
            .apply_auth(self)
            .send()
            .and_then(|mut resp| resp.json())
            .map_err(|e| e.into())
    }

    pub fn collection(
        &self,
        uid: Option<u64>,
    ) -> impl Future<Item = Vec<CollectionEntry>, Error = failure::Error> {
        let c = req::Client::new();
        let uid = uid.unwrap_or(self.settings.auth().as_ref().unwrap().info.user_id);
        c.get(&format!(
            "{}/user/{}/collection?cat=all_watching",
            API_ROOT!(),
            uid
        ))
        .apply_auth(self)
        .send()
        .and_then(|mut resp| resp.json())
        .map_err(|e| e.into())
    }
}
