use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RefreshInfo {}

#[derive(Serialize, Deserialize, Debug)]
pub struct AuthInfo {
    access_token: String,
    user_id: u64,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AppCred {
    client_id: String,
    client_secret: String,
}

impl AppCred {
    pub fn new(id: String, secret: String) -> AppCred {
        AppCred {
            client_id: id,
            client_secret: secret,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "grant_type")]
pub enum AuthPayload {
    #[serde(rename = "authorization_code")]
    AuthorizationCode {
        #[serde(flatten)]
        app_cred: AppCred,
        code: String,
        redirect_uri: String,
        state: Option<String>,
    },

    #[serde(rename = "refresh_token")]
    RefreshToken {
        #[serde(flatten)]
        app_cred: AppCred,
        refresh_token: String,
        redirect_uri: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RespError {
    error: String,
    error_description: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum AuthResp {
    Success(AuthInfo),
    Error(RespError),
}
