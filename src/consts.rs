macro_rules! BGM_ROOT {
    () => {
        "https://bgm.tv"
    };
}

pub(crate) const OAUTH_AUTHORIZE: &'static str = concat!(BGM_ROOT!(), "/oauth/authorize");
pub(crate) const OAUTH_ACCESS_TOKEN: &'static str = concat!(BGM_ROOT!(), "/oauth/access_token");
