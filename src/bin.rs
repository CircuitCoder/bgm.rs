use bgmtv::auth::{request_code, request_token, AppCred};
use bgmtv::settings::Settings;
use futures::future::Future;
use tokio;
use dirs;
use std::path::{Path, PathBuf};
use std::convert::AsRef;
use std::io::{Write, BufRead};
use colored::*;

fn default_path() -> impl AsRef<Path> {
    let mut buf = dirs::config_dir().unwrap_or(PathBuf::from("."));
    buf.push("bgmtty.toml");
    match buf.canonicalize() {
        Ok(can) => can,
        Err(_) => buf,
    }
}

fn init_credentials() {
    println!("{}", "bgmTTY runs as a OAuth client of bgm.tv. Hence we require a valid app credential.".blue().bold());
    println!("{}", "You may create a new app at https://bgm.tv/dev/app, or use pre-existing ones.".blue().bold());

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let lock = stdin.lock();
    let outlock = stdout.lock();

    let mut lines = lock.lines();
    print!("Please input your client ID: ");
    std::io::stdout().flush().expect("Could not flush stdout???");
    let id = lines.next();
    print!("Please input your client secret: ");
    std::io::stdout().flush().expect("Could not flush stdout???");
    let secret = lines.next();

    if id.is_none() || secret.is_none() {
        println!("Aborted!");
        return;
    }

    let id = id.unwrap().unwrap();
    let secret = secret.unwrap().unwrap();

    let cred = AppCred::new(id, secret);
    let settings = Settings::new(cred, None);
    let path = default_path();
    let parent = path.as_ref().parent();

    if let Some(parent) = parent {
        std::fs::create_dir_all(parent);
    }

    settings.save_to(path).expect("Failed to save config!");

    print!("{}", "Done! Now you can run bgmTTY again without the --init flag to perform a OAuth login.".green().bold())
}

fn main() {
    init_credentials();
    /*
    let fut = request_code(CLIENT_ID)
        .map_err(|e| println!("{:#?}", e))
        .and_then(|(code, redirect)| {
            println!("Code: {}, redirect: {}", code, redirect);
            request_token(
                AppCred::new(CLIENT_ID.to_owned(), CLIENT_SECRET.to_owned()),
                code,
                redirect,
            )
            .map_err(|e| println!("{}", e))
        })
        .map(|resp| println!("{:#?}", resp));
    tokio::run(fut);
    */
}
