use crate::auth::{refresh_token, AppCred, AuthInfo, AuthResp, RespError};
use chrono;
use failure::Error;
use futures::future::Future;
use serde_derive::{Deserialize, Serialize};
use serde_yaml;
use std::convert::AsRef;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

const REFRESH_RATIO: f64 = 0.2;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthHandle {
    info: AuthInfo,

    #[serde(with = "chrono::serde::ts_seconds")]
    time: chrono::DateTime<chrono::Utc>,
    redirect: String,
}

impl AuthHandle {
    fn time_diff(&self) -> i64 {
        let cur = chrono::Utc::now();
        cur.timestamp() - self.time.timestamp()
    }

    pub fn outdated(&self) -> bool {
        self.time_diff() as u64 > self.info.expires_in
    }

    pub fn requires_refresh(&self) -> bool {
        self.time_diff() as f64 > self.info.expires_in as f64 * REFRESH_RATIO
    }

    pub fn refresh(
        self,
        cred: AppCred,
    ) -> impl Future<Item = Result<AuthHandle, RespError>, Error = reqwest::Error> {
        let redir = self.redirect.clone();
        refresh_token(cred, self.info.refresh_token, self.redirect).map(|resp| match resp {
            AuthResp::Error(err) => Err(err),
            AuthResp::Success(info) => Ok(AuthHandle {
                info: info,
                time: chrono::Utc::now(),
                redirect: redir,
            }),
        })
    }

    pub fn redir(&self) -> &str {
        &self.redirect
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Settings {
    credentials: AppCred,
    auth: Option<AuthHandle>,
}

impl Settings {
    pub fn new(credentials: AppCred, auth: Option<AuthHandle>) -> Settings {
        Settings {
            credentials: credentials,
            auth: auth,
        }
    }

    pub fn load_from<P: AsRef<Path>>(file: P) -> Result<Settings, Error> {
        let mut buf = String::new();
        File::open(file)?.read_to_string(&mut buf);

        let settings: Settings = serde_yaml::from_str(&buf)?;

        Ok(settings)
    }

    pub fn save_to<P: AsRef<Path>>(&self, file: P) -> Result<(), Error> {
        let serialized = serde_yaml::to_vec(self)?;
        let mut f = File::create(file)?;
        f.write_all(&serialized)?;

        Ok(())
    }

    pub fn cred(&self) -> &AppCred {
        &self.credentials
    }

    pub fn auth(&self) -> &Option<AuthHandle> {
        &self.auth
    }

    pub fn update_auth(self, auth: AuthInfo, redirect: String) -> Settings {
        self.update_handle(AuthHandle {
            info: auth,
            time: chrono::Utc::now(),
            redirect: redirect,
        })
    }

    pub fn update_handle(self, handle: AuthHandle) -> Settings {
        Settings {
            credentials: self.credentials,
            auth: Some(handle),
        }
    }
}
