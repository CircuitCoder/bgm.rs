use crate::settings::Settings;
use futures::prelude::*;
use reqwest::r#async as req;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use url::form_urlencoded;

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
    pub id: u64,
    pub air_date: String,
    pub air_weekday: u8,

    pub name: String,
    pub name_cn: String,
    pub summary: String,

    #[serde(rename = "type")]
    pub subject_type: SubjectType,

    pub url: String,

    pub vols_count: Option<u64>,
    pub eps_count: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollectionEntry {
    pub ep_status: u64,
    pub vol_status: u64,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub lasttouch: chrono::DateTime<chrono::Utc>,

    pub subject: SubjectSmall,
}

impl CollectionEntry {
    pub fn step_ep(&self, dist: i64) -> u64 {
        if dist < 0 {
            if self.ep_status < (-dist) as u64 {
                0
            } else {
                self.ep_status - ((-dist) as u64)
            }
        } else {
            let pending = self.ep_status + dist as u64;
            match self.subject.eps_count {
                Some(e) if e < pending => e,
                _ => pending,
            }
        }
    }

    pub fn step_vol(&self, dist: i64) -> u64 {
        if dist < 0 {
            if self.vol_status < (-dist) as u64 {
                0
            } else {
                self.vol_status - ((-dist) as u64)
            }
        } else {
            let pending = self.vol_status + dist as u64;
            match self.subject.vols_count {
                Some(e) if e < pending => e,
                _ => pending,
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum CollectionStatus {
    #[serde(rename = "wish")]
    Wished,

    #[serde(rename = "collect")]
    Done,

    #[serde(rename = "do")]
    Doing,

    #[serde(rename = "on_hold")]
    OnHold,

    #[serde(rename = "dropped")]
    Dropped,
}

impl CollectionStatus {
    fn id(&self) -> &'static str {
        use crate::client::CollectionStatus::*;
        match self {
            Wished => "wish",
            Done => "collect",
            Doing => "do",
            OnHold => "on_hold",
            Dropped => "dropped",
        }
    }
}

impl Default for CollectionStatus {
    fn default() -> Self {
        CollectionStatus::Doing
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollectionDetail {
    pub status: CollectionStatus,
    pub rating: u8,
    pub comment: String,
    pub tag: Vec<String>,
}

#[derive(Serialize)]
struct ProgressPayload {
    pub watched_eps: String,
    pub watched_vols: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum APIResp<T> {
    Error {
        code: u16,
        error: String,
    },
    Success(T),
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

#[derive(Deserialize, Clone)]
pub struct SearchResultRaw {
    #[serde(rename = "results")]
    pub count: usize,
    pub list: Option<Vec<SubjectSmall>>,
}

#[derive(Clone)]
pub struct SearchResult {
    pub count: usize,
    pub list: Vec<SubjectSmall>,
}

impl Default for SearchResult {
    fn default() -> Self {
        Self {
            count: 0,
            list: Vec::new(),
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

    pub fn collection_detail(
        &self,
        id: u64,
    ) -> impl Future<Item = Option<CollectionDetail>, Error = failure::Error> {
        let c = req::Client::new();
        c.get(&format!(
            "{}/collection/{}",
            API_ROOT!(),
            id
        ))
        .apply_auth(self)
        .send()
        .and_then(|mut resp| resp.json())
        .map(|resp: APIResp<CollectionDetail>| {
            match resp {
                APIResp::Error{ .. } => None, // TODO: handle other errors
                APIResp::Success(payload) => Some(payload),
            }
        })
        .map_err(|e| e.into())
    }

    pub fn update_collection_detail(
        &self,
        id: u64,
        status: CollectionStatus,
        aux: Option<CollectionDetail>,
    ) -> impl Future<Item = CollectionDetail, Error = failure::Error> {
        let c = req::Client::new();

        let mut payload = HashMap::new();
        payload.insert("status", status.id().to_string());
        if let Some(content) = aux {
            payload.insert("rating", content.rating.to_string());
            payload.insert("comment", content.comment);
            payload.insert("tags", content.tag.join(","));
        }

        c.post(&format!(
            "{}/collection/{}/update",
            API_ROOT!(),
            id,
        ))
        .form(&payload)
        .apply_auth(self)
        .send()
        .and_then(|mut resp| resp.json())
        .map_err(|e| e.into())
    }

    pub fn subject(
        &self,
        id: u64,
    ) -> impl Future<Item = SubjectSmall, Error = failure::Error> {
        let c = req::Client::new();
        c.get(&format!(
            "{}/subject/{}",
            API_ROOT!(),
            id
        ))
        .apply_auth(self)
        .send()
        .and_then(|mut resp| resp.json())
        .map_err(|e| e.into())
    }

    pub fn progress(&self, coll: &CollectionEntry, ep: Option<u64>, vol: Option<u64>) -> impl Future<Item = (), Error = failure::Error> {
        let ep = ep.unwrap_or(coll.ep_status);
        let vol = vol.unwrap_or(coll.vol_status);

        let payload = ProgressPayload {
            watched_eps: ep.to_string(),
            watched_vols: if coll.subject.subject_type == SubjectType::Book {
                Some(vol.to_string())
            } else {
                None
            },
        };

        let c = req::Client::new();
        c.post(&format!(
            "{}/subject/{}/update/watched_eps",
            API_ROOT!(),
            coll.subject.id,
        ))
        .apply_auth(self)
        .form(&payload)
        .send()
        .map(|_| ()) // TODO: handle response
        .map_err(|e| e.into())
    }

    pub fn search(&self, keywords: &str, len: usize, skip: usize) -> impl Future<Item = SearchResult, Error = failure::Error> {
        let keywords = itertools::join(form_urlencoded::byte_serialize(keywords.as_bytes()), "");

        let c = req::Client::new();
        c.get(&format!(
            "{}/search/subject/{}?start={}&max_results={}",
            API_ROOT!(),
            keywords,
            skip,
            len,
        ))
        .apply_auth(self)
        .send()
        .and_then(|mut resp| resp.json())
        .map(|resp: APIResp<SearchResultRaw>| {
            match resp {
                APIResp::Success(r) => SearchResult{
                    count: r.count,
                    list: r.list.unwrap_or_else(Vec::new),
                },
                APIResp::Error{ .. } => SearchResult::default(),
            }
        })
        .map_err(|e| e.into())
    }
}
