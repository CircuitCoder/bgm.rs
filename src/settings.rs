use serde_derive::{Deserialize, Serialize};
use crate::auth::{AuthInfo, AppCred};
use chrono;
use serde_yaml;
use std::path::Path;
use std::convert::AsRef;
use failure::Error;
use std::fs::File;
use std::io::{Read, Write};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthHandle {
    info: AuthInfo,

    #[serde(with="chrono::serde::ts_seconds")]
    time: chrono::DateTime<chrono::Utc>,
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

    pub fn update_auth(self, auth: AuthInfo) -> Settings {
        let handle = AuthHandle {
            info: auth,
            time: chrono::Utc::now(),
        };

        Settings {
            credentials: self.credentials,
            auth: Some(handle),
        }
    }
}
