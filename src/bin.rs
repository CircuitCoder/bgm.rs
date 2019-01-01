#![feature(const_slice_len)]
#![feature(const_fn)]

mod widgets;
mod state;
mod help;
use crate::widgets::*;
use crate::state::*;
use crate::help::*;

use bgmtv::auth::{request_code, request_token, AppCred, AuthResp};
use bgmtv::client::{Client, CollectionStatus, SubjectType};
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
use std::sync::{Arc, Mutex};

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
    print!("请输入您的 Client secret: ");
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
    let (uri, fut) = request_code(cred.get_client_id());

    println!("请在本机使用浏览器前往 {} 完成验证", uri);

    let fut = fut
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
                "看上去这是您第一次使用 bgmTTY，或者"
                    .yellow()
                    .bold()
            );
            println!(
                "{}",
                "bgmTTY 没法打开配置文件。\n"
                    .yellow()
                    .bold()
            );

            println!("您可以带参数 --init 启动 bgmTTY 来创建一个新的配置文件，或者");
            println!(
                "将已有的配置文件放到 {}",
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
        println!("{}", "验证失败！可能是风把网线刮断了？".red().bold());
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
    fn padding_left(self, p: u16) -> Self;
    fn center(&self, width: u16, height: u16) -> Self;
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

    fn padding_left(mut self, p: u16) -> Self {
        self.x += p;
        if self.width > p {
            self.width -= p;
        } else {
            self.width = 0;
        }
        self
    }

    fn center(&self, mut width: u16, mut height: u16) -> Self {
        if width > self.width {
            width = self.width;
        }
        if height > self.height {
            height = self.height;
        }

        let left = (self.width - width) / 2;
        let top = (self.height - height) / 2;

        Self::new(left + self.x, top + self.y, width, height)
    }
}

trait CollectionStatusExt {
    fn disp(&self) -> &'static str;
    fn rotate(&self) -> Self;
}

impl CollectionStatusExt for CollectionStatus {
    fn disp(&self) -> &'static str {
        use bgmtv::client::CollectionStatus::*;
        match self {
            Wished => "打算做",
            Doing => "在做了",
            Done => "完成！",
            OnHold => "摸了",
            Dropped => "没得了",
        }
    }

    fn rotate(&self) -> Self {
        use bgmtv::client::CollectionStatus::*;
        match self {
            Wished => Doing,
            Doing => Done,
            Done => OnHold,
            OnHold => Dropped,
            Dropped => Wished,
        }
    }
}

trait SubjectTypeExt : Sized {
    fn disp(&self) -> &'static str;
}

impl SubjectTypeExt for SubjectType {
    fn disp(&self) -> &'static str {
        match self {
            SubjectType::Anime => "动画骗",
            SubjectType::Book => "书籍",
            SubjectType::Real => "三次元",
            SubjectType::Game => "游戏",
            SubjectType::Music => "音乐",
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

    let mut cursize = terminal.size()?;

    let (apptx, apprx) = unbounded();
    let (evtx, evrx) = unbounded();

    let stdin_lock = Arc::new(Mutex::new(()));

    kickoff_listener(evtx, stdin_lock.clone());

    let mut app = AppState::create(apptx, client);
    let mut ui = UIState::with(stdin_lock);

    loop {
        // Process Splits

        use tui::layout::*;
        use tui::widgets::*;

        if ui.pending == Some(PendingUIEvent::Quit) {
            break;
        }

        if ui.pending == Some(PendingUIEvent::Reset) {
            terminal.clear()?;
            terminal.hide_cursor()?;
            terminal.resize(cursize)?; // Clears buffer
        }

        // Safe catch, who knows how many racing conditions are there in the codebase?
        if ui.tabs.len() == 0 {
            break;
        }

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

                let mut help_scroll = help_scroll.scroll(ui.help_scroll.get());
                help_scroll.set_bound(help_inner);
                ui.help_scroll.set(help_scroll.get_scroll());
                help_scroll.render(&mut f, help_inner);

                if let Some(PendingUIEvent::Click(x, y, btn)) = pending {
                    if help_inner.contains(x, y) {
                        match help_scroll.intercept(x, y, btn) {
                            Some(ScrollEvent::ScrollTo(pos)) => {
                                ui.help_scroll.set(pos);
                            }
                            Some(ScrollEvent::ScrollUp) => {
                                ui.help_scroll.delta(-1);
                            }
                            Some(ScrollEvent::ScrollDown) => {
                                ui.help_scroll.delta(1);
                            }
                            _ => {}
                        }
                    }
                }

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
            let tab_names = ui.tabs.iter().map(|e| e.disp(&app)).collect::<Vec<_>>();
            let tab_name_borrows = tab_names.iter().map(|e| e.as_str()).collect::<Vec<_>>();
            let mut tabber = Tabber::with(tab_name_borrows.as_slice()).select(ui.tab);
            tabber.set_bound(tab_inner);
            tabber.render(&mut f, tab_inner);

            if let Some(PendingUIEvent::Click(x, y, btn)) = pending {
                if tab_inner.contains(x, y) {
                    match tabber.intercept(x, y, btn) {
                        Some(TabberEvent::Select(i)) => ui.select_tab(i),
                        Some(TabberEvent::Close(i)) => ui.close_tab(i),
                        _ => {}
                    }
                }
            }

            let needs_help = ui.needs_help();
            let status = ui.command.prompt().unwrap_or_else(|| if needs_help {
                "按 h 可以打开帮助哦".to_string()
            } else { app.last_message() });
            let mut status_line = CJKText::new(&status);
            let status_inner = chunks[2].padding_hoz(1);
            status_line.render(&mut f, status_inner);

            let is_double_click = ui.is_double_click();
            match ui.active_tab_mut() {
                Tab::Collection => {
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
                        .map(SubjectTypeExt::disp)
                        .collect::<Vec<&'static str>>();
                    let mut filters = FilterList::with(&filter_names, &ui.filters);

                    let collection = app.fetch_collection();

                    let count;
                    if let FetchResult::Direct(ref collection) = collection {
                        count = SELECTS.iter().map(|t| {
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
                        ui.focus.set_limit(collection.len());

                        let inner = outer.inner(subchunks[1]);

                        let mut scroll = Scroll::default();

                        let collection = Some(collection);
                        let mut ents = ui
                            .do_filter(&collection)
                            .map(ViewingEntry::with_coll)
                            .collect::<Vec<_>>();

                        if let Some(i) = ui.focus.get() {
                            ents[i].select(true);
                        }

                        for ent in ents.iter_mut() {
                            scroll.push(ent);
                        }

                        let mut scroll = scroll.scroll(ui.scroll.get());
                        scroll.set_bound(inner);

                        // Update offset
                        ui.scroll.set(scroll.get_scroll());

                        scroll.render(&mut f, inner);

                        if let Some(PendingUIEvent::ScrollIntoView(index)) = pending {
                            scroll.scroll_into_view(index);
                            ui.scroll.set(scroll.get_scroll());
                        }

                        if let Some(PendingUIEvent::Click(x, y, btn)) = pending {
                            if inner.contains(x, y) {
                                match scroll.intercept(x, y, btn) {
                                    Some(ScrollEvent::ScrollTo(pos)) => {
                                        ui.scroll.set(pos);
                                    }
                                    Some(ScrollEvent::ScrollUp) => {
                                        ui.scroll.delta(-1);
                                    }
                                    Some(ScrollEvent::ScrollDown) => {
                                        ui.scroll.delta(1);
                                    }
                                    Some(ScrollEvent::Sub(i)) => match ents[i].intercept(x, y, btn)
                                    {
                                        Some(ViewingEntryEvent::Click) => {
                                            if ui.focus.get() == Some(i) && is_double_click {
                                                ui.goto_detail(collection.unwrap()[i].subject.id);
                                            } else {
                                                ui.focus.set(Some(i));
                                            }
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

                Tab::Search{ ref text } => {
                    let mut block = Block::default().borders(Borders::ALL ^ Borders::TOP);
                    block.render(&mut f, chunks[1]);
                    SingleCell::new(tui::symbols::line::VERTICAL_RIGHT).render(&mut f, Rect::new(chunks[1].x, chunks[1].y-1, 1, 1));
                    SingleCell::new(tui::symbols::line::VERTICAL_LEFT).render(&mut f, Rect::new(chunks[1].x + chunks[1].width - 1, chunks[1].y-1, 1, 1));
                    let inner = block.inner(chunks[1]);

                    let input = inner.center(inner.width - 2, 5);
                    let mut input_block = Block::default().borders(Borders::ALL);
                    input_block.render(&mut f, input);
                    let input_inner = input_block.inner(input).inner(1);

                    let mut text_comp = if text != "" {
                        let mut text_comp = CJKText::new(text);
                        text_comp.set_style(tui::style::Style::default().fg(tui::style::Color::White));
                        text_comp
                    } else {
                        CJKText::new("按 e 或 Enter 开始输入，然后双击 Enter 搜索")
                    };
                    text_comp.render(&mut f, input_inner);
                }

                Tab::Subject{ id, scroll: ref mut scroll_val } => {
                    let mut block = Block::default().borders(Borders::ALL ^ Borders::TOP);
                    block.render(&mut f, chunks[1]);
                    SingleCell::new(tui::symbols::line::VERTICAL_RIGHT).render(&mut f, Rect::new(chunks[1].x, chunks[1].y-1, 1, 1));
                    SingleCell::new(tui::symbols::line::VERTICAL_LEFT).render(&mut f, Rect::new(chunks[1].x + chunks[1].width - 1, chunks[1].y-1, 1, 1));
                    let inner = block.inner(chunks[1]).padding_left(1);

                    use tui::style::*;

                    let detail = app.fetch_collection_detail(*id);
                    let subject = app.fetch_subject(*id);

                    match detail + subject {
                        FetchResult::Deferred => {
                            let text = format!("猫咪检索中... ID: {}", id);
                            CJKText::new(&text).render(&mut f, inner);
                        }
                        FetchResult::Direct((detail, subject)) => {
                            let mut scroll = Scroll::default();

                            let mut subject_text = CJKText::raw([
                                (subject.name.as_str(), Style::default().fg(Color::Yellow)),
                                ("\n", Style::default()),
                                (subject.name_cn.as_str(), Style::default().fg(Color::White)),
                                ("\n\n", Style::default()),
                                (subject.summary.as_str(), Style::default()),
                                ("\n\n", Style::default()),
                            ].to_vec());

                            scroll.push(&mut subject_text);

                            let status;
                            let score;
                            let tag;
                            let mut detail_cont;
                            let mut detail_text;
                            let mut comment;

                            if let Some(detail) = detail {
                                detail_cont = detail;
                                status = detail_cont.status.disp();
                                score = if detail_cont.rating == 0 {
                                    "未评分".to_string()
                                } else {
                                    format!("{} / 10", detail_cont.rating)
                                };
                                tag = detail_cont.tag.join(", ");

                                detail_text = CJKText::raw([
                                    ("状态: ", Style::default().fg(Color::Blue)),
                                    (status, Style::default()),

                                    ("\n", Style::default()),

                                    ("评分: ", Style::default().fg(Color::Blue)),
                                    (&score, Style::default()),

                                    ("\n", Style::default()),

                                    ("标签: ", Style::default().fg(Color::Blue)),
                                    (&tag, Style::default()),

                                    ("\n\n", Style::default()),
                                    ("评论: ", Style::default().fg(Color::Blue)),
                                ].to_vec());

                                comment = CJKText::new(&detail_cont.comment);

                                scroll.push(&mut detail_text);
                                scroll.push(&mut comment);
                            } else {
                                detail_text = CJKText::raw([
                                    ("状态: ", Style::default().fg(Color::Blue)),
                                    ("没打算", Style::default()),
                                ].to_vec());

                                scroll.push(&mut detail_text);
                            }

                            let mut scroll = scroll.scroll(scroll_val.get());
                            scroll.set_bound(inner);
                            scroll_val.set(scroll.get_scroll());

                            scroll.set_bound(inner);
                            scroll.render(&mut f, inner);

                            if let Some(PendingUIEvent::Click(x, y, btn)) = pending {
                                if inner.contains(x, y) {
                                    match scroll.intercept(x, y, btn) {
                                        Some(ScrollEvent::ScrollTo(pos)) => {
                                            scroll_val.set(pos);
                                        }
                                        Some(ScrollEvent::ScrollUp) => {
                                            scroll_val.delta(-1);
                                        }
                                        Some(ScrollEvent::ScrollDown) => {
                                            scroll_val.delta(1);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }

                Tab::SearchResult{ ref search, index, scroll: ref mut scroll_val, ref mut focus } => {
                    let mut block = Block::default().borders(Borders::ALL ^ Borders::TOP);
                    block.render(&mut f, chunks[1]);
                    SingleCell::new(tui::symbols::line::VERTICAL_RIGHT).render(&mut f, Rect::new(chunks[1].x, chunks[1].y-1, 1, 1));
                    SingleCell::new(tui::symbols::line::VERTICAL_LEFT).render(&mut f, Rect::new(chunks[1].x + chunks[1].width - 1, chunks[1].y-1, 1, 1));
                    let inner = block.inner(chunks[1]);


                    match app.fetch_search(search, *index) {
                        FetchResult::Deferred => {
                            let region = inner.inner(1);
                            Paragraph::new([Text::raw("Loading...")].iter())
                                .alignment(Alignment::Center)
                                .wrap(true)
                                .render(&mut f, region);
                        }
                        FetchResult::Direct(result) => {
                            use tui::style::*;

                            focus.set_limit(result.list.len());

                            let mut scroll = Scroll::default();
                            let count = result.count.to_string();
                            let visible = result.list.len().to_string();
                            let lower = (*index * SEARCH_PAGING + 1).to_string();
                            let upper = std::cmp::min(result.count as usize, (1+*index) * SEARCH_PAGING).to_string();

                            let mut heading = if result.count == 0 {
                                CJKText::raw([
                                    (search.as_str(), Style::default().fg(Color::Green)),
                                    ("\n", Style::default()),
                                    ("这里是", Style::default()),
                                    ("没有猫咪", Style::default().fg(Color::Yellow)),
                                    ("的荒原\n\n是不是越界了?", Style::default()),
                                ].to_vec())
                            } else {
                                CJKText::raw([
                                    (search.as_str(), Style::default().fg(Color::Green)),
                                    ("\n", Style::default()),
                                    (count.as_str(), Style::default().fg(Color::Yellow)),
                                    (" 结果，", Style::default()),
                                    (lower.as_str(), Style::default().fg(Color::Yellow)),
                                    (" - ", Style::default()),
                                    (upper.as_str(), Style::default().fg(Color::Yellow)),
                                    ("，", Style::default()),
                                    (visible.as_str(), Style::default().fg(Color::Yellow)),
                                    (" 可见", Style::default()),
                                ].to_vec())
                            };

                            scroll.push(&mut heading);

                            let mut ents = result.list.iter().map(ViewingEntry::with_subject).collect::<Vec<_>>();

                            if let Some(focus) = focus.get().and_then(|focus| ents.get_mut(focus)) {
                                focus.select(true);
                            }

                            for ent in ents.iter_mut() {
                                scroll.push(ent);
                            }

                            let inner = inner.padding_left(1);

                            let mut scroll = scroll.scroll(scroll_val.get());
                            scroll.set_bound(inner);
                            scroll_val.set(scroll.get_scroll());

                            scroll.render(&mut f, inner);

                            if let Some(PendingUIEvent::ScrollIntoView(index)) = pending {
                                scroll.scroll_into_view(index+1);
                                scroll_val.set(scroll.get_scroll());
                            }

                            if let Some(PendingUIEvent::Click(x, y, btn)) = pending {
                                if inner.contains(x, y) {
                                    match scroll.intercept(x, y, btn) {
                                        Some(ScrollEvent::ScrollTo(pos)) => {
                                            scroll_val.set(pos);
                                        }
                                        Some(ScrollEvent::ScrollUp) => {
                                            scroll_val.delta(-1);
                                        }
                                        Some(ScrollEvent::ScrollDown) => {
                                            scroll_val.delta(1);
                                        }
                                        Some(ScrollEvent::Sub(i)) if i > 0 => match ents[i-1].intercept(x, y, btn) {
                                            Some(ViewingEntryEvent::Click) => {
                                                if focus.get() == Some(i-1) && is_double_click {
                                                    ui.goto_detail(result.list[i-1].id);
                                                } else {
                                                    focus.set(Some(i-1));
                                                }
                                            }
                                            _ => {}
                                        },
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
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


fn kickoff_listener(tx: Sender<UIEvent>, stdin_lock: Arc<Mutex<()>>) {
    use std::io;
    use std::thread;
    use termion::event::Event;
    use termion::input::TermRead;

    thread::spawn(move || {
        let stdin = io::stdin();
        let control_sequence_backoff = std::time::Duration::new(0, 5000000);
        let mut last_backoff = None;

        for ev in stdin.events() {
            if let Ok(ev) = ev {
                if last_backoff.is_some()
                    && last_backoff.unwrap() + control_sequence_backoff > std::time::Instant::now() {
                    continue;
                }

                let result = match ev {
                    Event::Key(key) => tx.send(UIEvent::Key(key)),
                    Event::Mouse(mouse) => tx.send(UIEvent::Mouse(mouse)),
                    Event::Unsupported(_) => {
                        last_backoff = Some(std::time::Instant::now());
                        Ok(())
                    }
                };

                if let Err(e) = result {
                    println!("{}", e);
                }
            }
            { let _guard = stdin_lock.lock().unwrap(); }
        }
    });
}
