#![feature(const_slice_len)]
#![feature(arbitrary_self_types)]

pub mod events;
use crate::events::{Event, Events};

use bgmtv::auth::{request_code, request_token, AppCred, AuthResp};
use bgmtv::client::{Client, CollectionEntry, SubjectType, User};
use bgmtv::settings::Settings;
use clap;
use colored::*;
use crossbeam_channel::{unbounded, Select, Sender};
use dirs;
use failure::Error;
use futures::future::Future;
use std::convert::AsRef;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use termion;
use termion::raw::IntoRawMode;
use tokio;
use tui;

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
    println!(
        "{}",
        "bgmTTY runs as a OAuth client of bgm.tv. Hence we require a valid app credential."
            .blue()
            .bold()
    );
    println!(
        "{}",
        "You may create a new app at https://bgm.tv/dev/app, or use pre-existing ones."
            .blue()
            .bold()
    );

    let stdin = std::io::stdin();
    let lock = stdin.lock();

    let mut lines = lock.lines();
    print!("Please input your client ID: ");
    std::io::stdout()
        .flush()
        .expect("Could not flush stdout???");
    let id = lines.next();
    print!("Please input your client secret: ");
    std::io::stdout()
        .flush()
        .expect("Could not flush stdout???");
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

    settings
        .save_to(path)
        .expect(&"Failed to save config!".red().bold());

    print!(
        "{}",
        "Done! Now you can run bgmTTY again without the --init flag to perform a OAuth login."
            .green()
            .bold()
    )
}

fn new_auth(settings: Settings) -> Result<Settings, ()> {
    let set = settings.clone();
    let cred = set.cred().clone();
    let fut = request_code(cred.get_client_id())
        .map_err(|e| println!("{:#?}", e))
        .and_then(|(code, redirect)| {
            println!("Code: {}, redirect: {}", code, redirect);
            request_token(cred, code, redirect.clone())
                .map_err(|e| println!("{}", e))
                .map(|resp| (resp, redirect))
        })
        .and_then(|(resp, redirect)| match resp {
            AuthResp::Success(info) => {
                let newset = set.update_auth(info, redirect);
                newset
                    .save_to(default_path())
                    .expect(&"Failed to save config!".red().bold());
                futures::future::ok(newset)
            }
            _ => {
                println!(
                    "{}",
                    &"Refresh failed! Please check your OAuth credentials and try again."
                        .red()
                        .bold()
                );
                futures::future::err(())
            }
        });
    let mut runtime = tokio::runtime::Runtime::new().expect("Unable to create tokio runtime!");
    runtime.block_on(fut)
}

fn refresh_auth(settings: Settings) -> Result<Settings, ()> {
    let set = settings.clone();
    let cred = set.cred().clone();

    let fut = settings
        .auth()
        .clone()
        .unwrap()
        .refresh(cred)
        .map_err(|e| println!("{}", e))
        .and_then(|resp| match resp {
            Ok(handle) => {
                let newset = set.update_handle(handle);
                newset
                    .save_to(default_path())
                    .expect(&"Failed to save config!".red().bold());
                futures::future::ok(newset)
            }
            _ => {
                println!(
                    "{}",
                    &"Refresh failed! Please check your OAuth credentials and try again."
                        .red()
                        .bold()
                );
                futures::future::err(())
            }
        });
    let mut runtime = tokio::runtime::Runtime::new().expect("Unable to create tokio runtime!");
    runtime.block_on(fut)
}

fn main() {
    let matches = clap::App::new("bgmTTY")
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("bgm.tv on term")
        .arg(
            clap::Arg::with_name("init")
                .long("init")
                .help("(Re)initialize OAuth credentials"),
        )
        .arg(
            clap::Arg::with_name("refresh")
                .long("refresh")
                .help("Force refresh OAuth tokens"),
        )
        .get_matches();

    if matches.is_present("init") {
        init_credentials();
        std::process::exit(0);
    }

    let settings = match load_settings() {
        Ok(set) => set,
        Err(e) => {
            println!("{}", e);
            println!(
                "{}",
                "It seems that you are running bgmTTY for the first time, or"
                    .yellow()
                    .bold()
            );
            println!(
                "{}",
                "at least we cannot read your config file.\n"
                    .yellow()
                    .bold()
            );

            println!("Rerun bgmTTY with flag --init to create the config file, or");
            println!(
                "get a copy from your previous computer and put it to {}",
                default_path().as_ref().to_str().unwrap()
            );
            std::process::exit(1);
        }
    };

    let settings = if let Some(auth) = settings.auth() {
        if auth.outdated() {
            new_auth(settings)
        } else if auth.requires_refresh() || matches.is_present("refresh") {
            refresh_auth(settings)
        } else {
            Ok(settings)
        }
    } else {
        new_auth(settings)
    };

    let settings = if let Ok(s) = settings {
        s
    } else {
        println!("{}", "Unalbe to authenticate!".red().bold());
        std::process::exit(1);
    };

    println!("{:#?}", settings);
    let client = Client::new(settings);
    bootstrap(client);
}

enum FetchResult<T> {
    Direct(T),
    Deferred,
}

struct AppStateInner {
    notifier: Sender<()>,

    user: Option<User>,
    collections: Option<Vec<CollectionEntry>>,
}

struct AppState {
    client: Client,

    inner: Arc<Mutex<AppStateInner>>,

    rt: tokio::runtime::Runtime,

    fetching_collection: bool,
}

impl AppState {
    fn create(notifier: Sender<()>, client: Client) -> AppState {
        AppState {
            client: client,

            inner: Arc::new(Mutex::new(AppStateInner {
                notifier: notifier,
                user: None,
                collections: None,
            })),

            rt: tokio::runtime::Runtime::new().expect("Cannot create runtime!"),

            fetching_collection: false,
        }
    }

    fn fetch_collection(&mut self) -> FetchResult<Vec<CollectionEntry>> {
        if self.fetching_collection {
            if let Some(ref entries) = self.inner.lock().unwrap().collections {
                return FetchResult::Direct(entries.clone());
            } else {
                return FetchResult::Deferred;
            }
        }

        self.fetching_collection = true;

        let fut = self.client.collection(None);
        let handle = self.inner.clone();

        let fut = fut
            .map(move |resp| {
                let mut inner = handle.lock().unwrap();

                inner.collections = Some(resp);
                inner.notifier.send(());
            })
            .map_err(|e| println!("{}", e));

        self.rt.spawn(fut);
        return FetchResult::Deferred;
    }
}

const TABS: [&'static str; 1] = ["Collections"];
const SELECTS: [(&'static str, SubjectType); 3] = [
    ("Anime", SubjectType::Anime),
    ("Book", SubjectType::Book),
    ("Real", SubjectType::Real),
];

struct UIState {
    tab: usize,
    filter: [bool; SELECTS.len()],
}

impl Default for UIState {
    fn default() -> UIState {
        UIState {
            tab: 0,
            filter: [true; SELECTS.len()],
        }
    }
}

impl UIState {
    fn next_tab(&mut self) {
        if self.tab != TABS.len() - 1 {
            self.tab += 1;
        }
    }

    fn prev_tab(&mut self) {
        if self.tab != 0 {
            self.tab -= 1;
        }
    }
}

fn bootstrap(client: Client) -> Result<(), failure::Error> {
    let stdout = std::io::stdout().into_raw_mode()?;
    let stdout = termion::input::MouseTerminal::from(stdout);
    let stdout = termion::screen::AlternateScreen::from(stdout);
    let backend = tui::backend::TermionBackend::new(stdout);
    let mut terminal = tui::Terminal::new(backend)?;

    terminal.hide_cursor()?;

    let mut cursize = tui::layout::Rect::default();

    let (apptx, apprx) = unbounded();

    let events = Events::new();
    let mut app = AppState::create(apptx, client);
    let mut ui = UIState::default();

    loop {
        let size = terminal.size()?;
        if cursize != size {
            terminal.resize(size)?;
            cursize = size;
        }

        terminal.draw(|mut f| {
            use tui::layout::*;
            use tui::style::*;
            use tui::widgets::*;

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
                .split(cursize);

            Tabs::default()
                .block(Block::default().borders(Borders::ALL).title("bgmTTY"))
                .titles(&TABS)
                .style(Style::default().fg(Color::Green))
                .select(0)
                .render(&mut f, chunks[0]);

            match ui.tab {
                0 => {
                    // Render collections
                    let subchunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Min(20), Constraint::Percentage(100)].as_ref())
                        .split(chunks[1]);

                    SelectableList::default()
                        .block(Block::default().title("Filter").borders(Borders::ALL))
                        .items(
                            &SELECTS
                                .iter()
                                .map(|(name, _)| *name)
                                .collect::<Vec<&'static str>>(),
                        )
                        .select(Some(1))
                        .style(Style::default().fg(Color::White))
                        .highlight_style(Style::default().modifier(Modifier::Italic))
                        .highlight_symbol(">>")
                        .render(&mut f, subchunks[0]);

                    let collection = app.fetch_collection();

                    if let FetchResult::Direct(collection) = collection {
                        let para = collection
                            .iter()
                            .map(|e| format!("{}\n", e.subject.name))
                            .map(|e| Text::raw(e))
                            .collect::<Vec<_>>();
                        Paragraph::new(para.iter())
                            .block(Block::default().borders(Borders::ALL))
                            .alignment(Alignment::Left)
                            .wrap(true)
                            .render(&mut f, subchunks[1]);
                    } else {
                        Paragraph::new([Text::raw("Loading...")].iter())
                            .block(Block::default().borders(Borders::ALL))
                            .alignment(Alignment::Center)
                            .wrap(true)
                            .render(&mut f, subchunks[1]);
                    };
                }
                _ => {}
            }
        })?;

        use termion::event::Key;

        let mut select = Select::new();

        select.recv(&events.rx);
        select.recv(&apprx);

        let oper = select.select();
        let index = oper.index();

        if index == 0 {
            let event = oper.recv(&events.rx).unwrap();

            match event {
                Event::Input(input) => match input {
                    Key::Char('q') => {
                        break;
                    }
                    _ => {}
                },
                _ => {}
            }
        } else {
            oper.recv(&apprx).unwrap();
        }
    }

    Ok(())
}
