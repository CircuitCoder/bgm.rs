use crate::auth::{refresh_token, AppCred, AuthInfo, AuthResp, RespError};
use chrono;
use futures::future::Future;
use serde_derive::{Deserialize, Serialize};

const REFRESH_RATIO: f64 = 0.2;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthHandle {
    pub(crate) info: AuthInfo,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub(crate) time: chrono::DateTime<chrono::Utc>,
    pub(crate) redirect: String,
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

    pub fn cred(&self) -> &AppCred {
        &self.credentials
    }

    pub fn auth(&self) -> &Option<AuthHandle> {
        &self.auth
    }

    pub fn logout(self) -> Settings {
        Settings {
            credentials: self.credentials,
            auth: None,
        }
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
