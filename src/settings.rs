use serde_derive::{Deserialize, Serialize};
use crate::auth::{AuthInfo, AppCred};
use chrono;
use toml;
use std::path::Path;
use std::convert::AsRef;
use failure::Error;
use std::fs::File;
use std::io::{Read, Write};

#[derive(Serialize, Deserialize, Debug)]
pub struct AuthHandle {
    info: AuthInfo,

    #[serde(with="chrono::serde::ts_seconds")]
    time: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
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

        let settings: Settings = toml::from_str(&buf)?;

        Ok(settings)
    }

    pub fn save_to<P: AsRef<Path>>(&self, file: P) -> Result<(), Error> {
        let serialized = toml::to_vec(self)?;
        let mut f = File::create(file)?;
        f.write_all(&serialized)?;

        Ok(())
    }
}
