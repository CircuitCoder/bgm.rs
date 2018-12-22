use bgmtv::auth::{request_code, request_token, AppCred, AuthResp};
use bgmtv::settings::Settings;
use futures::future::Future;
use tokio;
use dirs;
use std::path::{Path, PathBuf};
use std::convert::AsRef;
use std::io::{Write, BufRead};
use colored::*;
use clap;
use failure::Error;

fn default_path() -> impl AsRef<Path> {
    let mut buf = dirs::config_dir().unwrap_or(PathBuf::from("."));
    buf.push("bgmtty.yml");
    match buf.canonicalize() {
        Ok(can) => can,
        Err(_) => buf,
    }
}

fn load_settings() -> Result<Settings, Error> {
    Settings::load_from(default_path())
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
        std::fs::create_dir_all(parent).expect(&"Permission denied!".red().bold());
    }

    settings.save_to(path).expect(&"Failed to save config!".red().bold());

    print!("{}", "Done! Now you can run bgmTTY again without the --init flag to perform a OAuth login.".green().bold())
}

fn main() {
    let matches = clap::App::new("bgmTTY")
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("bgm.tv on term")
        .arg(clap::Arg::with_name("init")
             .long("init")
             .help("(Re)initialize OAuth credentials"))
        .get_matches();

    if matches.is_present("init") {
        init_credentials();
        std::process::exit(0);
    }

    let settings = match load_settings() {
        Ok(set) => set,
        Err(_) => {
            println!("{}", "It seems that you are running bgmTTY for the first time, or".yellow().bold());
            println!("{}", "at least we cannot read your config file.\n".yellow().bold());

            println!("Rerun bgmTTY with flag --init to create the config file, or");
            println!("get a copy from your previous computer and put it to {}", default_path().as_ref().to_str().unwrap());
            std::process::exit(1);
        }
    };

    let set = settings.clone();
    let cred = set.cred().clone();
    let fut = request_code(cred.get_client_id())
        .map_err(|e| println!("{:#?}", e))
        .and_then(|(code, redirect)| {
            println!("Code: {}, redirect: {}", code, redirect);
            request_token(
                cred,
                code,
                redirect,
            )
            .map_err(|e| println!("{}", e))
        })
        .map(|resp| {
            println!("{:#?}", resp);
            match resp {
                AuthResp::Success(info) => {
                    let newset = set.update_auth(info);
                    newset.save_to(default_path()).expect(&"Failed to save config!".red().bold());
                },
                _ => {
                    println!("{}", &"Login failed! Please check your OAuth credentials and try again.".red().bold())
                }
            }
        });
    tokio::run(fut);
}
