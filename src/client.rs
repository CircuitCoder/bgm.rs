use crate::settings::Settings;

pub struct Client {
    settings: Settings,
}

impl Client {
    pub fn new(settings: Settings) -> Client {
        Client { settings: settings }
    }
}
