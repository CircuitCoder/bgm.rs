#![feature(const_slice_len)]
#![feature(arbitrary_self_types)]
#![feature(fnbox)]

mod widgets;
mod state;
mod help;
use crate::widgets::*;
use crate::state::*;
use crate::help::*;

use bgmtv::auth::{request_code, request_token, AppCred, AuthResp};
use bgmtv::client::Client;
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
        "bgmTTY 通过 OAuth 协议向 bgm.tv 申请验证，所以我们需要有效的 OAuth 应用凭证。"
            .blue()
            .bold()
    );
    println!(
        "{}",
        "您可以前往 https://bgm.tv/dev/app 进行申请, 或者使用既有的凭证。"
            .blue()
            .bold()
    );

    let stdin = std::io::stdin();
    let lock = stdin.lock();

    let mut lines = lock.lines();
    print!("请输入您的 Client ID: ");
    std::io::stdout()
        .flush()
        .expect("Could not flush stdout???");
    let id = lines.next();
    print!("请输入您的 Client secret");
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
        "完成了！现在您可以去掉 --init 参数重新启动 bgmTTY，进行 OAuth 认证。"
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
                    &"获取 Token 失败！请检查您的 Client ID/secret 并重试。"
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
                    &"刷新 Token 失败！请检查您的 Client ID/secret 并重试。"
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
                .help("(重新)初始化 OAuth 应用凭证"),
        )
        .arg(
            clap::Arg::with_name("refresh")
                .long("refresh")
                .help("强制刷新 OAuth Token"),
        )
        .arg(
            clap::Arg::with_name("logout")
                .long("logout")
                .help("登出账户并立即退出"),
        )
        .arg(
            clap::Arg::with_name("auth-only")
                .long("auth-only")
                .help("仅进行认证或刷新 Token"),
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

    if matches.is_present("logout") {
        settings.logout()
            .save_to(default_path())
            .expect(&"Failed to save config!".red().bold());

        return;
    }

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

    if matches.is_present("auth-only") {
        return;
    }

    let client = Client::new(settings);
    bootstrap(client).expect("Terminal failed");
}

trait RectExt {
    fn contains(&self, x: u16, y: u16) -> bool;
    fn padding_hoz(self, p: u16) -> Self;
}

impl RectExt for tui::layout::Rect {
    fn contains(&self, x: u16, y: u16) -> bool {
        return x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height;
    }

    fn padding_hoz(mut self, p: u16) -> Self {
        self.x += p;
        if self.width >= p * 2 {
            self.width -= p * 2;
        } else {
            self.width = 0;
        }
        self
    }
}

fn bootstrap(client: Client) -> Result<(), failure::Error> {
    let stdout = std::io::stdout().into_raw_mode()?;
    let stdout = termion::input::MouseTerminal::from(stdout);
    let stdout = termion::screen::AlternateScreen::from(stdout);
    let backend = tui::backend::TermionBackend::new(stdout);
    let mut terminal = tui::Terminal::new(backend)?;

    terminal.hide_cursor()?;

    let mut cursize = terminal.size()?;

    let (apptx, apprx) = unbounded();
    let (evtx, evrx) = unbounded();

    kickoff_listener(evtx);

    let mut app = AppState::create(apptx, client);
    let mut ui = UIState::default();

    'main: loop {
        // Process Splits

        use tui::layout::*;
        use tui::widgets::*;

        terminal.draw(|mut f| {
            let pending = ui.pending.clone();

            let primary_chunk = if ui.help {
                let primary_split = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(80),
                        Constraint::Percentage(20),
                    ].as_ref())
                    .split(cursize);

                let mut help_block = Block::default().borders(Borders::LEFT);
                help_block.render(&mut f, primary_split[1]);
                let help_inner = help_block.inner(primary_split[1]);
                let mut help_texts = HELP_DATABASE
                    .iter()
                    .filter(|e| e.pred()(&ui))
                    .map(Into::into)
                    .collect::<Vec<CJKText>>();
                let mut help_scroll = Scroll::default();

                for text in help_texts.iter_mut() {
                    help_scroll.push(text);
                }

                help_scroll.render(&mut f, help_inner);

                primary_split[0]
            } else {
                cursize
            };

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ].as_ref())
                .split(primary_chunk);

            let mut tab_block = Block::default().borders(Borders::ALL).title("bgmTTY");
            tab_block.render(&mut f, chunks[0]);
            let tab_inner = tab_block.inner(chunks[0]);
            let mut tabber = Tabber::with(&TABS).select(ui.tab);
            tabber.set_bound(tab_inner);
            tabber.render(&mut f, tab_inner);

            if let Some(PendingUIEvent::Click(x, y, btn)) = pending {
                if tab_inner.contains(x, y) {
                    match tabber.intercept(x, y, btn) {
                        Some(TabberEvent::Select(i)) => ui.select_tab(i),
                        _ => {}
                    }
                }
            }

            let status = ui.command.prompt().unwrap_or_else(|| app.last_message());
            let mut status_line = CJKText::new(&status);
            let status_inner = chunks[2].padding_hoz(1);
            status_line.render(&mut f, status_inner);

            match ui.tab {
                0 => {
                    // Render collections
                    let subchunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Min(20), Constraint::Percentage(100)].as_ref())
                        .split(chunks[1]);

                    let mut filter_block = Block::default().borders(Borders::ALL ^ Borders::TOP);
                    filter_block.render(&mut f, subchunks[0]);
                    // Draw custom corners
                    SingleCell::new(tui::symbols::line::VERTICAL_RIGHT).render(&mut f, Rect::new(subchunks[0].x, subchunks[0].y-1, 1, 1));
                    SingleCell::new(tui::symbols::line::HORIZONTAL_DOWN).render(&mut f, Rect::new(subchunks[0].x + subchunks[0].width - 1, subchunks[0].y-1, 1, 1));
                    SingleCell::new(tui::symbols::line::HORIZONTAL_UP).render(&mut f, Rect::new(subchunks[0].x + subchunks[0].width - 1, subchunks[0].y+subchunks[0].height-1, 1, 1));
                    let filter_inner = filter_block.inner(subchunks[0]).padding_hoz(1);
                    let filter_names = SELECTS
                        .iter()
                        .map(|(name, _)| *name)
                        .collect::<Vec<&'static str>>();
                    let mut filters = FilterList::with(&filter_names, &ui.filters);

                    let collection = app.fetch_collection();

                    let count;
                    if let FetchResult::Direct(ref collection) = collection {
                        count = SELECTS.iter().map(|(_, t)| {
                            let mut c = 0;
                            for ent in collection {
                                if &ent.subject.subject_type == t {
                                    c += 1;
                                }
                            }

                            c
                        }).collect::<Vec<usize>>();

                        filters = filters.counting(&count);
                    }
                    filters.set_bound(filter_inner);
                    filters.render(&mut f, filter_inner);

                    if let Some(PendingUIEvent::Click(x, y, btn)) = pending {
                        if filter_inner.contains(x, y) {
                            match filters.intercept(x, y, btn) {
                                Some(FilterListEvent::Toggle(i)) => {
                                    ui.toggle_filter(i, &collection.clone().into())
                                }
                                _ => {}
                            }
                        }
                    }

                    let mut outer = Block::default().borders(Borders::ALL ^ Borders::TOP ^ Borders::LEFT);
                    outer.render(&mut f, subchunks[1]);
                    SingleCell::new(tui::symbols::line::VERTICAL_LEFT).render(&mut f, Rect::new(subchunks[1].x + subchunks[1].width - 1, subchunks[1].y-1, 1, 1));

                    if let FetchResult::Direct(collection) = collection {
                        // Sync app state into ui state
                        ui.set_focus_limit(collection.len());

                        let inner = outer.inner(subchunks[1]);

                        let mut scroll = Scroll::default();

                        let collection = Some(collection);
                        let mut ents = ui
                            .do_filter(&collection)
                            .map(ViewingEntry::new)
                            .collect::<Vec<_>>();

                        if let Some(i) = ui.focus {
                            ents[i].select(true);
                        }

                        for ent in ents.iter_mut() {
                            scroll.push(ent);
                        }

                        let mut scroll = scroll.scroll(ui.scroll);
                        scroll.set_bound(inner);

                        // Update offset
                        ui.set_scroll(scroll.get_scroll());

                        scroll.render(&mut f, inner);

                        if let Some(PendingUIEvent::ScrollIntoView(index)) = pending {
                            scroll.scroll_into_view(index);
                            ui.set_scroll(scroll.get_scroll());
                        }

                        if let Some(PendingUIEvent::Click(x, y, btn)) = pending {
                            if inner.contains(x, y) {
                                match scroll.intercept(x, y, btn) {
                                    Some(ScrollEvent::ScrollTo(pos)) => {
                                        ui.set_scroll(pos);
                                    }
                                    Some(ScrollEvent::ScrollUp) => {
                                        ui.scroll_delta(-1);
                                    }
                                    Some(ScrollEvent::ScrollDown) => {
                                        ui.scroll_delta(1);
                                    }
                                    Some(ScrollEvent::Sub(i)) => match ents[i].intercept(x, y, btn)
                                    {
                                        Some(ViewingEntryEvent::Click) => {
                                            ui.set_focus(Some(i));
                                        }
                                        _ => {}
                                    },
                                    _ => {}
                                }
                            }
                        }
                    } else {
                        let region = outer.inner(subchunks[1]).inner(1);

                        Paragraph::new([Text::raw("Loading...")].iter())
                            .alignment(Alignment::Center)
                            .wrap(true)
                            .render(&mut f, region);
                    };
                }
                _ => {}
            }
        })?;

        if ui.clear_pending() {
            continue;
        }

        loop {
            let mut select = Select::new();

            select.recv(&evrx);
            select.recv(&apprx);

            let result = select.select_timeout(std::time::Duration::from_millis(5));
            if let Ok(oper) = result {
                let index = oper.index();

                if index == 0 {
                    let event = oper.recv(&evrx).unwrap();
                    ui.reduce(event, &mut app);
                    if ui.pending == Some(PendingUIEvent::Quit) {
                        break 'main;
                    }
                } else {
                    oper.recv(&apprx).unwrap();
                }

                break;
            };

            // Check for terminal size
            let size = terminal.size()?;
            if cursize != size {
                terminal.resize(size)?;
                cursize = size;

                // Proceed to repaint
                break;
            }
        }
    }

    Ok(())
}

fn kickoff_listener(tx: Sender<UIEvent>) {
    use std::io;
    use std::thread;
    use termion::event::Event;
    use termion::input::TermRead;

    thread::spawn(move || {
        let stdin = io::stdin();
        for ev in stdin.events() {
            if let Ok(ev) = ev {
                let result = match ev {
                    Event::Key(key) => tx.send(UIEvent::Key(key)),
                    Event::Mouse(mouse) => tx.send(UIEvent::Mouse(mouse)),
                    _ => Ok(()),
                };

                if let Err(e) = result {
                    println!("{}", e);
                }
            }
        }
    });
}
