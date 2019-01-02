use bgmtv::client::{CollectionEntry, CollectionDetail, CollectionStatus, SubjectType, SubjectSmall, Client};
use crossbeam_channel::{Sender};
use std::sync::{Arc, Mutex};
use futures::future::Future;
use crate::CollectionStatusExt;
use std::io::{Read, Write};
use std::ops::Deref;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use std::collections::hash_map;

#[derive(Clone)]
pub enum FetchResult<T> {
    Direct(T),
    Deferred,
}

impl<T> FetchResult<T> {
    pub fn join<U>(self, another: FetchResult<U>) -> FetchResult<(T, U)> {
        match self {
            FetchResult::Deferred => FetchResult::Deferred,
            FetchResult::Direct(t) => match another {
                FetchResult::Deferred => FetchResult::Deferred,
                FetchResult::Direct(u) => FetchResult::Direct((t, u)),
            }
        }
    }
}

impl<T, U> std::ops::Add<FetchResult<U>> for FetchResult<T> {
    type Output = FetchResult<(T, U)>;

    fn add(self, other: FetchResult<U>) -> FetchResult<(T, U)> {
        self.join(other)
    }
}

#[derive(PartialEq, Clone)]
pub enum InnerState<I, T> {
    Fetching(I),
    Fetched(I, T),
    Discarded,
}

impl<T> Into<Option<T>> for FetchResult<T> {
    fn into(self) -> Option<T> {
        match self {
            FetchResult::Direct(c) => Some(c),
            FetchResult::Deferred => None,
        }
    }
}

#[derive(Clone)]
pub struct ShallowSearchResult {
    count: usize,
    ids: Vec<u64>,
}

pub struct PopulatedSearchResult {
    pub count: usize,
    pub list: Vec<SubjectSmall>,
}

pub const SEARCH_PAGING: usize = 10;

struct AppStateInner {
    notifier: Sender<()>,

    collection: InnerState<(), Vec<CollectionEntry>>,
    collection_detail: HashMap<u64, InnerState<(), Option<CollectionDetail>>>,
    subject: HashMap<u64, InnerState<(), SubjectSmall>>,
    search: HashMap<(String, usize), InnerState<(), ShallowSearchResult>>,

    messages: Vec<String>,
}

pub struct AppState {
    client: Client,

    inner: Arc<Mutex<AppStateInner>>,

    rt: tokio::runtime::Runtime,

    fetching_collection: bool,
}

impl AppState {
    pub fn create(notifier: Sender<()>, client: Client) -> AppState {
        AppState {
            client,

            inner: Arc::new(Mutex::new(AppStateInner {
                notifier,
                collection: InnerState::Discarded,
                collection_detail: HashMap::new(),
                subject: HashMap::new(),
                search: HashMap::new(),
                messages: ["Loading bgmTTY...".to_string()].to_vec(),
            })),

            rt: tokio::runtime::Runtime::new().expect("Cannot create runtime!"),

            fetching_collection: false,
        }
    }

    pub fn fetch_collection(&mut self) -> FetchResult<Vec<CollectionEntry>> {
        let mut guard = self.inner.lock().unwrap();
        if self.fetching_collection {
            match guard.collection {
                InnerState::Fetched(_, ref entries) =>
                    return FetchResult::Direct(entries.clone()),
                InnerState::Fetching(_) =>
                    return FetchResult::Deferred,
                _ => {
                    // Else: discarded, restart fetch
                    guard.collection = InnerState::Fetching(());
                }
            }
        }

        self.fetching_collection = true;
        guard.messages.push("刷新收藏中...".to_string());
        guard.notifier.send(()).unwrap();
        drop(guard);

        let fut = self.client.collection(None);
        let handle = self.inner.clone();
        let err_handle = self.inner.clone();

        let fut = fut
            .map(move |resp| {
                let mut inner = handle.lock().unwrap();

                inner.collection = InnerState::Fetched((), resp);
                inner.messages.push("收藏加载完成！".to_string());
                inner
                    .notifier
                    .send(())
                    .expect("Unable to notify the main thread");
            })
            .map_err(move |e| err_handle.lock().unwrap().messages.push(format!("请求失败！{}", e)));

        self.rt.spawn(fut);

        FetchResult::Deferred
    }

    pub fn update_progress(&mut self, coll: &CollectionEntry, ep: Option<u64>, vol: Option<u64>) {
        let mut guard = self.inner.lock().unwrap();
        guard.messages.push(format!("更新进度: {}...", coll.subject.id));
        guard.notifier.send(()).unwrap();

        let fut = self.client.progress(coll, ep, vol);
        let handle = self.inner.clone();
        let err_handle = self.inner.clone();

        let fut = fut
            .map(move |_| {
                let mut inner = handle.lock().unwrap();

                inner.collection = InnerState::Discarded;
                inner
                    .notifier
                    .send(())
                    .expect("Unable to notify the main thread");
            })
            .map_err(move |e| err_handle.lock().unwrap().messages.push(format!("请求失败！{}", e)));

        self.rt.spawn(fut);
    }

    pub fn publish_message(&mut self, msg: String) {
        let msgs = &mut self.inner.lock().unwrap().messages;
        msgs.push(msg);
    }

    pub fn last_message(&self) -> String {
        let msgs = &self.inner.lock().unwrap().messages;
        msgs[msgs.len()-1].clone()
    }

    pub fn fetch_collection_detail(&mut self, id: u64) -> FetchResult<Option<CollectionDetail>> {
        let mut guard = self.inner.lock().unwrap();
        let entry = guard.collection_detail.entry(id);
        match entry {
            hash_map::Entry::Vacant(entry) => { entry.insert(InnerState::Fetching(())); }
            hash_map::Entry::Occupied(mut entry) =>
                match entry.get_mut() {
                    InnerState::Fetched(_, ref result) =>
                        return FetchResult::Direct(result.clone()),
                    InnerState::Fetching(_) =>
                        return FetchResult::Deferred,
                    value => {
                        // Else: discarded or fetching another, restart fetch
                        *value = InnerState::Fetching(());
                    }
                }
        }

        guard.messages.push("获取收藏状态...".to_string());
        guard.notifier.send(()).unwrap();
        drop(guard);

        let fut = self.client.collection_detail(id);
        let handle = self.inner.clone();
        let err_handle = self.inner.clone();

        let fut = fut
            .map(move |resp| {
                let mut inner = handle.lock().unwrap();

                inner.collection_detail.insert(id, InnerState::Fetched((), resp));
                inner.messages.push("收藏加载完成！".to_string());
                inner
                    .notifier
                    .send(())
                    .expect("Unable to notify the main thread");
            })
            .map_err(move |e| err_handle.lock().unwrap().messages.push(format!("请求失败！{}", e)));

        self.rt.spawn(fut);

        FetchResult::Deferred
    }

    pub fn update_collection_detail(&mut self, id: u64, status: CollectionStatus, original: Option<CollectionDetail>) {
        let mut guard = self.inner.lock().unwrap();
        guard.messages.push("更新更新...".to_string());
        guard.notifier.send(()).unwrap();
        drop(guard);

        let fut = self.client.update_collection_detail(id, status, original);
        let handle = self.inner.clone();
        let err_handle = self.inner.clone();

        let fut = fut
            .map(move |resp| {
                let mut inner = handle.lock().unwrap();

                inner.collection_detail.insert(id, InnerState::Fetched((), Some(resp)));
                inner.messages.push("收藏更新完成！".to_string());
                inner
                    .notifier
                    .send(())
                    .expect("Unable to notify the main thread");
            })
            .map_err(move |e| err_handle.lock().unwrap().messages.push(format!("请求失败！{}", e)));

        self.rt.spawn(fut);
    }

    pub fn fetch_subject(&mut self, id: u64) -> FetchResult<SubjectSmall> {
        let mut guard = self.inner.lock().unwrap();
        let entry = guard.subject.entry(id);
        match entry {
            hash_map::Entry::Vacant(entry) => { entry.insert(InnerState::Fetching(())); }
            hash_map::Entry::Occupied(mut entry) =>
                match entry.get_mut() {
                    InnerState::Fetched(_, ref result) =>
                        return FetchResult::Direct(result.clone()),
                    InnerState::Fetching(_) =>
                        return FetchResult::Deferred,
                    value => {
                        // Else: discarded or fetching another, restart fetch
                        *value = InnerState::Fetching(());
                    }
                }
        }

        guard.messages.push(format!("获取条目中: {}...", id));
        guard.notifier.send(()).unwrap();
        drop(guard);

        let fut = self.client.subject(id);
        let handle = self.inner.clone();
        let err_handle = self.inner.clone();

        let fut = fut
            .map(move |resp| {
                let mut inner = handle.lock().unwrap();

                inner.subject.insert(id, InnerState::Fetched((), resp));
                inner.messages.push("条目加载完成！".to_string());
                inner
                    .notifier
                    .send(())
                    .expect("Unable to notify the main thread");
            })
            .map_err(move |e| err_handle.lock().unwrap().messages.push(format!("请求失败！{}", e)));

        self.rt.spawn(fut);

        FetchResult::Deferred
    }

    fn populate_search<'a>(&self, shallow: &'a ShallowSearchResult) -> FetchResult<PopulatedSearchResult> {
        let mut list = Vec::with_capacity(shallow.ids.len());
        let count = shallow.count;

        let guard = self.inner.lock().unwrap();

        for id in shallow.ids.iter() {
            if let Some(InnerState::Fetched((), ref content)) = guard.subject.get(id) {
                list.push(content.clone());
            } else {
                return FetchResult::Deferred;
            }
        }

        FetchResult::Direct(PopulatedSearchResult{ count, list })
    }

    pub fn fetch_search(&mut self, search: &str, index: usize) -> FetchResult<PopulatedSearchResult> {
        let mut guard = self.inner.lock().unwrap();
        let entry = guard.search.entry((search.to_string(), index));
        match entry {
            hash_map::Entry::Vacant(entry) => { entry.insert(InnerState::Fetching(())); }
            hash_map::Entry::Occupied(mut entry) =>
                match entry.get_mut() {
                    InnerState::Fetched(_, ref result) => {
                        let cloned = result.clone();
                        drop(guard);
                        return self.populate_search(&cloned);
                    }
                    InnerState::Fetching(_) =>
                        return FetchResult::Deferred,
                    value => {
                        // Else: discarded or fetching another, restart fetch
                        *value = InnerState::Fetching(());
                    }
                }
        }

        guard.messages.push(format!("搜索中: {}...", search));
        guard.notifier.send(()).unwrap();
        drop(guard);

        let skip = index * SEARCH_PAGING;
        let fut = self.client.search(search, SEARCH_PAGING, skip);
        let handle = self.inner.clone();
        let err_handle = self.inner.clone();

        let search = search.to_string();

        let fut = fut
            .map(move |resp| {
                let mut inner = handle.lock().unwrap();

                let mut ids = Vec::with_capacity(resp.list.len());
                let count = resp.count;

                for subject in resp.list.into_iter() {
                    ids.push(subject.id);
                    inner.subject.insert(subject.id, InnerState::Fetched((), subject));
                }

                inner.search.insert((search, index), InnerState::Fetched((), ShallowSearchResult{ count, ids }));

                inner.messages.push("搜索完成！".to_string());
                inner
                    .notifier
                    .send(())
                    .expect("Unable to notify the main thread");
            })
            .map_err(move |e| err_handle.lock().unwrap().messages.push(format!("请求失败！{}", e)));

        self.rt.spawn(fut);

        FetchResult::Deferred
    }

    pub fn refresh_collection(&mut self) {
        self.inner.lock().unwrap().collection = InnerState::Discarded;
    }

    pub fn refresh_search(&mut self, search: String, index: usize) {
        self.inner.lock().unwrap().search.entry((search, index)).and_modify(|s| *s = InnerState::Discarded);
    }

    pub fn refresh_subject(&mut self, id: u64) {
        self.inner.lock().unwrap().subject.entry(id).and_modify(|s| *s = InnerState::Discarded);
    }

    pub fn refresh_collection_detail(&mut self, id: u64) {
        self.inner.lock().unwrap().collection_detail.entry(id).and_modify(|s| *s = InnerState::Discarded);
    }
}

pub const SELECTS: [SubjectType; 3] = [
    SubjectType::Anime,
    SubjectType::Book,
    SubjectType::Real,
];

#[derive(Clone)]
pub struct ScrollState {
    scroll: u16,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self { scroll: 0 }
    }
}

impl ScrollState {
    pub fn get(&self) -> u16 {
        self.scroll
    }

    pub fn set(&mut self, s: u16) {
        self.scroll = s;
    }

    pub fn delta(&mut self, delta: i16) {
        let new_scroll = self.scroll as i16 + delta;
        self.scroll = if new_scroll < 0 { 0 } else { new_scroll as u16 };
    }
}

#[derive(Default, Clone, PartialEq)]
pub struct FocusState {
    focus: Option<usize>,
    limit: usize,
}

impl FocusState {
    fn normalize(&mut self) {
        if let Some(f) = self.focus {
            if f >= self.limit {
                if self.limit == 0 {
                    self.focus = None;
                } else {
                    self.focus = Some(self.limit - 1);
                }
            }
        }
    }

    pub fn set(&mut self, focus: Option<usize>) {
        self.focus = focus;
        self.normalize();
    }

    pub fn get(&self) -> Option<usize> {
        self.focus
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        self.normalize();
    }

    pub fn next(&mut self) {
        match self.focus {
            Some(f) => self.set(Some(f+1)),
            None => self.set(Some(0)),
        }
    }

    pub fn prev(&mut self) {
        match self.focus {
            Some(f) if f > 0 => self.set(Some(f-1)),
            _ => {}
        }
    }
}

#[derive(Clone)]
pub enum Tab {
    Collection,

    Search{
        text: String,
    },

    Subject{
        id: u64,
        scroll: ScrollState,
    },

    SearchResult{
        search: String,
        index: usize,
        scroll: ScrollState,
        focus: FocusState,
    },
}

impl Tab {
    pub fn disp(&self, _app: &AppState) -> String {
        // TODO: truncate
        match self {
            Tab::Collection => "格子".to_string(),
            Tab::Search{ .. } => "搜索".to_string(),
            Tab::Subject{ id, .. } => format!("条目: {}", id),
            Tab::SearchResult{ search, index, .. } => format!("搜索: {} / {}", search, index+1),
        }
    }

    pub fn is_search(&self) -> bool {
        match self {
            Tab::Search{ .. }=> true,
            _ => false,
        }
    }

    pub fn is_collection(&self) -> bool {
        match self {
            Tab::Collection => true,
            _ => false,
        }
    }

    pub fn is_subject(&self) -> bool {
        match self {
            Tab::Subject{ .. } => true,
            _ => false,
        }
    }

    pub fn is_search_result(&self) -> bool {
        match self {
            Tab::SearchResult{ .. } => true,
            _ => false,
        }
    }
    pub fn subject_id(&self) -> Option<u64> {
        match self {
            Tab::Subject{ id, .. } => Some(*id),
            _ => None,
        }
    }

    pub fn get_focus(&self) -> Option<usize> {
        match self {
            Tab::SearchResult{ focus, .. } => focus.get(),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum UIEvent {
    Key(termion::event::Key),
    Mouse(termion::event::MouseEvent),
}

#[derive(Clone, PartialEq)]
pub enum PendingUIEvent {
    Click(u16, u16, termion::event::MouseButton),
    ScrollIntoView(usize),
    KBTabSelect, // Requires scroll
    Quit,
    Reset,
}

#[derive(Clone)]
pub enum LongCommand {
    Absent,
    Graphical,
    Command(String),
    Toggle,

    EditRating(u64, CollectionDetail, String),
    EditStatus(u64, Option<CollectionDetail>, CollectionStatus),

    SearchInput(String),
}

impl LongCommand {
    pub fn present(&self) -> bool {
        match self {
            LongCommand::Absent => false,
            _ => true,
        }
    }

    pub fn prompt(&self) -> Option<String> {
        match self {
            LongCommand::Absent => None,
            LongCommand::Graphical => Some("g".to_string()),
            LongCommand::Command(ref inner) => Some(format!(":{}", inner)),
            LongCommand::Toggle => Some("t".to_string()),
            LongCommand::EditRating(_, _, r) => Some(format!("评分 (1-10, 0=取消): {}", r)),
            LongCommand::EditStatus(_, _, s) => Some(format!("状态: {} [Tab]", s.disp())),
            LongCommand::SearchInput(ref inner) => Some(format!("搜索: {}", inner)),
        }
    }
}

const HELP_THRESHOLD: usize = 3;

pub struct UIState {
    pub(crate) tabs: Vec<Tab>,
    pub(crate) tab: usize,
    pub(crate) tab_scroll: ScrollState,

    // TODO: move to the collection tab
    pub(crate) filters: [bool; SELECTS.len()],
    pub(crate) scroll: ScrollState,
    pub(crate) focus: FocusState,

    pub(crate) pending: Option<PendingUIEvent>,

    pub(crate) help: bool,
    pub(crate) help_scroll: ScrollState,

    pub(crate) command: LongCommand,

    stdin_lock: Arc<Mutex<()>>,
    last_click_interval: Option<Duration>,
    last_click: Option<(u16, u16, Instant)>,

    last_input_meaningless: bool,
    meaningless_count: usize,
}

impl UIState {
    pub fn with(stdin_lock: Arc<Mutex<()>>) -> UIState {
        UIState {
            tabs: [
                Tab::Collection,
                Tab::Search{ text: String::new() },
            ].to_vec(),
            tab: 0,
            tab_scroll:Default::default(),

            filters: [true; SELECTS.len()],
            scroll: Default::default(),
            focus: Default::default(),

            pending: None,

            help: false,
            help_scroll: Default::default(),

            command: LongCommand::Absent,

            stdin_lock,
            last_click_interval: None,
            last_click: None,

            last_input_meaningless: false,
            meaningless_count: 0,
        }
    }

    pub fn rotate_tab(&mut self) {
        if self.tab != self.tabs.len() - 1 {
            self.tab += 1;
        } else {
            self.tab = 0;
        }
    }

    pub fn rotate_tab_rev(&mut self) {
        if self.tab != 0 {
            self.tab -= 1;
        } else {
            self.tab = self.tabs.len() - 1;
        }
    }

    pub fn select_tab(&mut self, mut tab: usize) {
        if tab >= self.tabs.len() {
            tab = self.tabs.len() - 1;
        }

        self.tab = tab;
    }

    pub fn open_tab(&mut self, tab: Tab, pos: Option<usize>) -> usize {
        let mut pos = pos.unwrap_or(self.tab + 1);

        if pos > self.tabs.len() {
            pos = self.tabs.len();
        }

        self.tabs.insert(pos, tab);
        pos
    }

    pub fn replace_tab(&mut self, tab: Tab) {
        self.tabs[self.tab] = tab;
    }

    pub fn move_tab(&mut self, mut dest: usize) -> usize {
        if dest > self.tabs.len() {
            dest = self.tabs.len();
        }

        let tab = self.tabs.remove(self.tab);
        if dest > self.tab {
            dest -= 1;
        }
        self.tabs.insert(dest, tab);
        
        dest
    }

    pub fn close_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            if self.tab == self.tabs.len() - 1 && self.tab != 0 {
                self.tab -= 1;
            }

            self.tabs.remove(index);
        }

        if self.tabs.len() == 0 {
            self.pending = Some(PendingUIEvent::Quit);
        }
    }

    pub fn active_tab(&self) -> &Tab {
        // This really should not break
        self.tabs.get(self.tab).unwrap()
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        // This really should not break
        self.tabs.get_mut(self.tab).unwrap()
    }

    pub fn toggle_filter(&mut self, index: usize, entries: &Option<Vec<CollectionEntry>>) {
        if index >= self.filters.len() {
            return;
        }

        // Get original index of the filter
        let original = self
            .focus
            .get()
            .and_then(|focus| self.do_filter(entries).skip(focus).next())
            .map(|e| e.subject.id);

        if let Some(f) = self.filters.get_mut(index) {
            *f = !*f;
        }

        let mut new_focus = None;
        for (i, content) in self.do_filter(entries).enumerate() {
            if Some(content.subject.id) == original {
                new_focus = Some(i);
            }
        }

        self.focus.set(new_focus);
    }

    pub fn do_filter<'s, 'a>(
        &'s self,
        entries: &'a Option<Vec<CollectionEntry>>,
    ) -> impl Iterator<Item = &'a CollectionEntry> {
        match entries {
            None => itertools::Either::Left(std::iter::empty()),
            Some(entries) => {
                let filters = self.filters.clone();
                itertools::Either::Right(entries.iter().filter(move |e| {
                    for (i, t) in SELECTS.iter().enumerate() {
                        if t == &e.subject.subject_type {
                            return filters[i];
                        }
                    }
                    return false;
                }))
            }
        }
    }

    pub fn reduce(&mut self, ev: UIEvent, app: &mut AppState) -> &mut Self {
        use termion::event::{Key, MouseEvent};

        if self.last_input_meaningless {
            self.meaningless_count += 1;
        } else {
            self.meaningless_count = 0;
        }

        self.last_input_meaningless = false;

        // Second: match long command input
        if self.command.present() {
            if ev == UIEvent::Key(Key::Esc) {
                self.command = LongCommand::Absent;
                return self;
            }

            match self.command {
                LongCommand::Graphical => {
                    match ev {
                        UIEvent::Key(Key::Char('t')) => {
                            self.rotate_tab();
                            self.pending = Some(PendingUIEvent::KBTabSelect);
                            self.command = LongCommand::Absent;
                            return self;
                        }
                        UIEvent::Key(Key::Char('T')) => {
                            self.rotate_tab_rev();
                            self.pending = Some(PendingUIEvent::KBTabSelect);
                            self.command = LongCommand::Absent;
                            return self;
                        }
                        UIEvent::Key(Key::Char('g')) => {
                            match self.active_tab_mut() {
                                Tab::Collection => {
                                    self.scroll.set(0);
                                    self.focus.set(Some(0));
                                }
                                Tab::Subject{ ref mut scroll, .. } => {
                                    scroll.set(0);
                                }
                                Tab::SearchResult{ ref mut scroll, ref mut focus, .. } => {
                                    scroll.set(0);
                                    focus.set(Some(0));
                                }
                                _ => {}
                            }
                            self.command = LongCommand::Absent;
                            return self;
                        }
                        UIEvent::Key(_) => {
                            self.command = LongCommand::Absent;
                            return self;
                        }
                        _ => {}
                    }
                }

                LongCommand::Command(ref mut cmd) => {
                    match ev {
                        UIEvent::Key(Key::Char('\n')) => {
                            match cmd as &str {
                                "qa" => self.pending = Some(PendingUIEvent::Quit),
                                "q" => {
                                    self.close_tab(self.tab);
                                    self.pending = Some(PendingUIEvent::KBTabSelect);
                                }
                                "help" => self.help = !self.help,
                                "tabe search" => self.tab = self.open_tab(Tab::Search{ text: String::new() }, None),
                                "tabe coll" => self.tab = self.open_tab(Tab::Collection, None),
                                ref e if e.starts_with("tabm ") => {
                                    let index = e[5..].parse::<usize>();
                                    match index {
                                        Ok(index) => {
                                            self.tab = self.move_tab(index);
                                            self.pending = Some(PendingUIEvent::KBTabSelect);
                                        }
                                        _ => app.publish_message(format!("{} 是不认识的数字!", &e[5..])),
                                    }
                                }
                                _ => app.publish_message("是不认识的命令!".to_string()),
                            }

                            self.command = LongCommand::Absent;
                            return self;
                        }
                        UIEvent::Key(Key::Backspace) => {
                            if cmd.pop().is_none() {
                                self.command = LongCommand::Absent;
                            }

                            return self;
                        }
                        UIEvent::Key(Key::Char(c)) => {
                            cmd.push(c);
                            return self
                        }
                        UIEvent::Key(_) => return self,
                        _ => {}
                    }
                }

                LongCommand::Toggle => {
                    match ev {
                        UIEvent::Key(Key::Char(i @ '1'...'9')) => {
                            let i = i.to_digit(10).unwrap() as usize;
                            let collection = app.fetch_collection().into();
                            self.toggle_filter(i-1, &collection);

                            self.command = LongCommand::Absent;
                            return self;
                        }
                        UIEvent::Key(_) => {
                            self.command = LongCommand::Absent;
                            return self;
                        }
                        _ => {}
                    }
                }

                LongCommand::EditRating(id, ref coll, ref mut rating) => {
                    match ev {
                        UIEvent::Key(Key::Char('\n')) => {
                            if let Ok(mut digit) = rating.parse::<u8>() {
                                if digit > 10 {
                                    digit = 10;
                                }

                                if coll.rating != digit {
                                    let mut coll = coll.clone();
                                    coll.rating = digit;
                                    app.update_collection_detail(id, coll.status.clone(), Some(coll));
                                }
                            }

                            self.command = LongCommand::Absent;
                            return self;
                        }
                        UIEvent::Key(Key::Backspace) => {
                            rating.pop();
                            return self;
                        }
                        UIEvent::Key(Key::Char(c @ '0'...'9')) => {
                            if rating == "" || (rating == "1" && c == '0') {
                                rating.push(c);
                            } else if rating == "0" {
                                *rating = c.to_string();
                            }
                            return self
                        }
                        UIEvent::Key(_) => return self,
                        _ => {}
                    }
                }

                LongCommand::EditStatus(id, ref coll, ref mut current) => {
                    match ev {
                        UIEvent::Key(Key::Char('\t')) => {
                            *current = current.rotate();
                            return self;
                        }
                        UIEvent::Key(Key::Char('\n')) => {
                            app.update_collection_detail(id, current.clone(), coll.clone());

                            self.command = LongCommand::Absent;
                            return self;
                        }
                        UIEvent::Key(_) => {
                            self.command = LongCommand::Absent;
                            return self;
                        }
                        _ => {}
                    }
                }

                LongCommand::SearchInput(ref mut staging) => {
                    match ev {
                        UIEvent::Key(Key::Char('\n')) => {
                            let cloned = staging.to_string();
                            if let Tab::Search{ ref mut text } = self.active_tab_mut() {
                                *text = cloned;
                            }
                            self.command = LongCommand::Absent;
                            return self;
                        }
                        UIEvent::Key(Key::Backspace) => {
                            staging.pop();
                            return self;
                        }
                        UIEvent::Key(Key::Char(c)) => {
                            staging.push(c);
                            return self
                        }
                        UIEvent::Key(_) => return self,
                        _ => {}
                    }
                }

                _ => {}
            }
        }

        // No long command transfer possible, proceed to normal dispatch

        match ev {
            UIEvent::Key(Key::Ctrl('q')) => self.pending = Some(PendingUIEvent::Quit),

            UIEvent::Key(Key::Down) | UIEvent::Key(Key::Char('j')) if self.active_tab().is_collection() => {
                self.focus.next();
                if let Some(f) = self.focus.get() {
                    self.pending = Some(PendingUIEvent::ScrollIntoView(f));
                }
            }
            UIEvent::Key(Key::Up) | UIEvent::Key(Key::Char('k')) if self.active_tab().is_collection() => {
                self.focus.prev();
                if let Some(f) = self.focus.get() {
                    self.pending = Some(PendingUIEvent::ScrollIntoView(f));
                }
            }
            UIEvent::Key(Key::Char('t')) if self.active_tab().is_collection() => {
                self.command = LongCommand::Toggle;
            }
            UIEvent::Key(Key::Char('+')) if self.active_tab().is_collection() && self.focus.get().is_some() => {
                let focus = self.focus.get().unwrap();
                let collection = app.fetch_collection().into();
                let target = self.do_filter(&collection).skip(focus).next();

                if let Some(t) = target {
                    let (ep, vol) = match t.subject.subject_type {
                        SubjectType::Book => (None, Some(t.step_vol(1))),
                        _ => (Some(t.step_ep(1)), None),
                    };

                    app.update_progress(t, ep, vol);
                }
            }
            UIEvent::Key(Key::Char('-')) if self.active_tab().is_collection() && self.focus.get().is_some() => {
                let focus = self.focus.get().unwrap();
                let collection = app.fetch_collection().into();
                let target = self.do_filter(&collection).skip(focus).next();

                if let Some(t) = target {
                    let (ep, vol) = match t.subject.subject_type {
                        SubjectType::Book => (None, Some(t.step_vol(-1))),
                        _ => (Some(t.step_ep(-1)), None),
                    };

                    app.update_progress(t, ep, vol);
                }
            }
            UIEvent::Key(Key::Char('\n')) if self.active_tab().is_collection() && self.focus.get().is_some() => {
                let focus = self.focus.get().unwrap();
                let collection = app.fetch_collection().into();
                let target = self.do_filter(&collection).skip(focus).next();

                if let Some(t) = target {
                    self.goto_detail(t.subject.id);
                }
            }
            UIEvent::Key(Key::Esc) if self.active_tab().is_collection() && self.focus.get().is_some() => self.focus.set(None),

            UIEvent::Key(Key::Char('s')) if self.active_tab().is_subject() => {
                let id = self.active_tab().subject_id().unwrap();
                if let FetchResult::Direct(coll) = app.fetch_collection_detail(id) {
                    let initial = if let Some(ref coll) = coll {
                        coll.status.clone()
                    } else {
                        Default::default()
                    };
                    self.command = LongCommand::EditStatus(id, coll, initial);
                }
            }

            UIEvent::Key(Key::Char('r')) if self.active_tab().is_subject() => {
                let id = self.active_tab().subject_id().unwrap();
                if let FetchResult::Direct(Some(coll)) = app.fetch_collection_detail(id) {
                    let rating = coll.rating.to_string();
                    self.command = LongCommand::EditRating(id, coll, rating);
                }
            }

            UIEvent::Key(Key::Char('t')) if self.active_tab().is_subject() => {
                let id = self.active_tab().subject_id().unwrap();
                if let FetchResult::Direct(Some(mut coll)) = app.fetch_collection_detail(id) {
                    let initial = coll.tag.join("\n");
                    if let Ok(Some(content)) = self.edit(&initial, app) {
                        let segs = content.lines().filter(|e| e.len() > 0).map(|e| e.to_string()).collect::<Vec<String>>();
                        coll.tag = segs;
                        app.update_collection_detail(id, coll.status.clone(), Some(coll));
                    }
                }
            }

            UIEvent::Key(Key::Char('c')) if self.active_tab().is_subject() => {
                let id = self.active_tab().subject_id().unwrap();
                if let FetchResult::Direct(Some(mut coll)) = app.fetch_collection_detail(id) {
                    if let Ok(Some(content)) = self.edit(&coll.comment, app) {
                        if content != coll.comment {
                            coll.comment = content;
                            app.update_collection_detail(id, coll.status.clone(), Some(coll));
                        }
                    }
                }
            }

            UIEvent::Key(Key::Down) | UIEvent::Key(Key::Char('j')) if self.active_tab().is_subject() =>
                if let Tab::Subject{ ref mut scroll, .. } = self.active_tab_mut() {
                    scroll.delta(1)
                }

            UIEvent::Key(Key::Up) | UIEvent::Key(Key::Char('k')) if self.active_tab().is_subject() =>
                if let Tab::Subject{ ref mut scroll, .. } = self.active_tab_mut() {
                    scroll.delta(-1)
                }

            UIEvent::Key(Key::Esc) if self.active_tab().is_subject() => self.close_tab(self.tab),

            UIEvent::Key(Key::Char('\n')) if self.active_tab().is_search() => {
                if let Tab::Search { ref text } = self.active_tab() {
                    if text == "" {
                        self.command = LongCommand::SearchInput(String::new());
                    } else {
                        self.replace_tab(Tab::SearchResult{
                            search: text.clone(),
                            index: 0,
                            scroll: Default::default(),
                            focus: Default::default(),
                        });
                    }
                }
            }

            UIEvent::Key(Key::Char('e')) if self.active_tab().is_search() => {
                if let Tab::Search { ref text } = self.active_tab() {
                    self.command = LongCommand::SearchInput(text.clone());
                }
            }

            UIEvent::Key(Key::Down) | UIEvent::Key(Key::Char('j')) if self.active_tab().is_search_result() =>
                if let Tab::SearchResult{ ref mut focus, .. } = self.active_tab_mut() {
                    focus.next();
                    if let Some(f) = focus.get() {
                        self.pending = Some(PendingUIEvent::ScrollIntoView(f));
                    }
                }

            UIEvent::Key(Key::Up) | UIEvent::Key(Key::Char('k')) if self.active_tab().is_search_result() =>
                if let Tab::SearchResult{ ref mut focus, .. } = self.active_tab_mut() {
                    focus.prev();
                    if let Some(f) = focus.get() {
                        self.pending = Some(PendingUIEvent::ScrollIntoView(f));
                    }
                }

            UIEvent::Key(Key::Char('\n')) if self.active_tab().is_search_result() && self.active_tab().get_focus().is_some() => {
                if let Tab::SearchResult{ ref search, index, ref focus, .. } = self.active_tab() {
                    let focus = focus.get().unwrap();
                    let result: Option<_> = app.fetch_search(search, *index).into();
                    let target = result.as_ref().and_then(|result: &PopulatedSearchResult| result.list.iter().skip(focus).next());

                    if let Some(t) = target {
                        self.goto_detail(t.id);
                    }
                }
            }

            UIEvent::Key(Key::Esc) if self.active_tab().is_search_result() && self.active_tab().get_focus().is_some() => {
                if let Tab::SearchResult{ ref mut focus, .. } = self.active_tab_mut() {
                    focus.set(None);
                }
            }

            UIEvent::Key(Key::Char('n')) if self.active_tab().is_search_result() => {
                if let Tab::SearchResult{ ref mut index, .. } = self.active_tab_mut() {
                    *index += 1;
                }
            }

            UIEvent::Key(Key::Char('N')) if self.active_tab().is_search_result() => {
                if let Tab::SearchResult{ ref mut index, .. } = self.active_tab_mut() {
                    if *index > 0 {
                        *index -= 1;
                    }
                }
            }

            UIEvent::Key(Key::Char('\t')) => self.rotate_tab(),
            UIEvent::Key(Key::Char('g')) => self.command = LongCommand::Graphical,
            UIEvent::Key(Key::Char('G')) => 
                match self.active_tab_mut() {
                    Tab::Collection => {
                        self.scroll.set(std::u16::MAX - 1000);
                        self.focus.set(Some(std::usize::MAX));
                    }
                    Tab::Subject{ ref mut scroll, .. } => {
                        scroll.set(std::u16::MAX - 1000);
                    }
                    Tab::SearchResult{ ref mut scroll, ref mut focus, .. } => {
                        scroll.set(std::u16::MAX - 1000);
                        focus.set(Some(std::usize::MAX));
                    }
                    _ => {}
                }
            UIEvent::Key(Key::Char('R')) => 
                match self.active_tab_mut() {
                    Tab::Collection => {
                        app.refresh_collection();
                    }
                    Tab::Subject{ id, .. } => {
                        app.refresh_subject(*id);
                        app.refresh_collection_detail(*id);
                    }
                    Tab::SearchResult{ ref search, index, .. } => {
                        app.refresh_search(search.clone(), *index);
                    }
                    _ => {}
                }
            UIEvent::Key(Key::Char(':')) => self.command = LongCommand::Command(String::new()),
            UIEvent::Key(Key::Char('?')) | UIEvent::Key(Key::Char('h')) => self.help = !self.help,
            UIEvent::Key(Key::Char('J')) if self.help => self.help_scroll.delta(1),
            UIEvent::Key(Key::Char('K')) if self.help => self.help_scroll.delta(-1),

            UIEvent::Mouse(m) => match m {
                MouseEvent::Press(btn, x, y) => {
                    self.pending = Some(PendingUIEvent::Click(x - 1, y - 1, btn));
                    self.update_click(x, y);
                }
                MouseEvent::Hold(x, y) => {
                    self.pending = Some(PendingUIEvent::Click(
                        x - 1,
                        y - 1,
                        termion::event::MouseButton::Left,
                    ));
                    self.last_click_interval = None;
                    self.last_click = None;
                }
                _ => {}
            },

            _ => {
                self.last_input_meaningless = true;
            }
        }

        self
    }

    pub fn clear_pending(&mut self) -> bool {
        if self.pending.is_some() {
            self.pending = None;
            true
        } else {
            false
        }
    }

    fn update_click(&mut self, x: u16, y: u16) {
        let now = Instant::now();
        if let Some((ox, oy, i)) = self.last_click {
            if ox == x && oy == y {
                self.last_click_interval = Some(now - i);
            } else {
                self.last_click_interval = None;
            }
        }
        self.last_click = Some((x, y, now));
    }

    pub fn is_double_click(&self) -> bool {
        self.last_click_interval.is_some()
            && self.last_click_interval.unwrap() < Duration::from_millis(300)
    }

    pub fn goto_detail(&mut self, id: u64) {
        for (i, t) in self.tabs.iter().enumerate() {
            if t.subject_id() == Some(id) {
                self.tab = i;
                return;
            }
        }

        self.tab = self.open_tab(Tab::Subject{ id, scroll: ScrollState::default() }, None);
    }

    pub fn needs_help(&self) -> bool {
        self.meaningless_count + 2 > HELP_THRESHOLD
    }

    /**
     * This method is intended to be called in the reducer.
     * Since the reducer runs in the main (UI) thread,
     * this will effectively blocks the rendering, so bgmTTY won't interfere with
     * whatever editor the user uses
     */
    pub fn edit(&mut self, content: &str, app: &mut AppState) -> std::io::Result<Option<String>>  {
        self.pending = Some(PendingUIEvent::Reset);

        let mut temp = tempfile::NamedTempFile::new()?;
        write!(temp, "{}", content)?;
        let path = temp.into_temp_path();

        let status = {
            let _guard = self.stdin_lock.lock().unwrap();
            let result = std::process::Command::new("vim").arg(path.deref()).status();
            if result.is_err() {
                app.publish_message("找不到编辑器啦！参数 -e 指定编辑器，或者试试 Vim 嘛？".to_string());
            }
            result?
        };

        if status.success() {
            let mut content = String::new();
            std::fs::File::open(path.deref())?.read_to_string(&mut content)?;
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }
}
