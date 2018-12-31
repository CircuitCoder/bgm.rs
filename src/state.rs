use bgmtv::client::{CollectionEntry, SubjectType, Client};
use crossbeam_channel::{Sender};
use std::sync::{Arc, Mutex};
use futures::future::Future;

#[derive(Clone)]
pub enum FetchResult<T> {
    Direct(T),
    Deferred,
}

pub enum InnerState<T> {
    Fetching,
    Fetched(T),
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

struct AppStateInner {
    notifier: Sender<()>,

    collections: InnerState<Vec<CollectionEntry>>,

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
                collections: InnerState::Fetching,
                messages: ["Loading bgmTTY...".to_string()].to_vec(),
            })),

            rt: tokio::runtime::Runtime::new().expect("Cannot create runtime!"),

            fetching_collection: false,
        }
    }

    pub fn fetch_collection(&mut self) -> FetchResult<Vec<CollectionEntry>> {
        let mut guard = self.inner.lock().unwrap();
        if self.fetching_collection {
            match guard.collections {
                InnerState::Fetched(ref entries) =>
                    return FetchResult::Direct(entries.clone()),
                InnerState::Fetching =>
                    return FetchResult::Deferred,
                _ => {
                    // Else: discarded, restart fetch
                    guard.collections = InnerState::Fetching;
                }
            }
        }

        self.fetching_collection = true;
        guard.messages.push("刷新收藏中...".to_string());
        drop(guard);

        let fut = self.client.collection(None);
        let handle = self.inner.clone();

        let fut = fut
            .map(move |resp| {
                let mut inner = handle.lock().unwrap();

                inner.collections = InnerState::Fetched(resp);
                inner.messages.push("收藏加载完成！".to_string());
                inner
                    .notifier
                    .send(())
                    .expect("Unable to notify the main thread");
            })
            .map_err(|e| println!("{}", e));

        self.rt.spawn(fut);

        FetchResult::Deferred
    }

    pub fn update_progress(&mut self, coll: &CollectionEntry, ep: Option<u64>, vol: Option<u64>) {
        let mut guard = self.inner.lock().unwrap();
        guard.messages.push(format!("更新进度: {}...", coll.subject.id));
        guard.notifier.send(()).unwrap();

        let fut = self.client.progress(coll, ep, vol);
        let handle = self.inner.clone();

        let fut = fut
            .map(move |_| {
                let mut inner = handle.lock().unwrap();

                inner.collections = InnerState::Discarded;
                inner
                    .notifier
                    .send(())
                    .expect("Unable to notify the main thread");
            })
            .map_err(|e| println!("{}", e));
        self.rt.spawn(fut);
    }

    pub fn last_message(&self) -> String {
        let msgs = &self.inner.lock().unwrap().messages;
        msgs[msgs.len()-1].clone()
    }
}

pub const TABS: [&str; 2] = ["格子", "搜索"];
pub const SELECTS: [(&str, SubjectType); 3] = [
    ("动画骗", SubjectType::Anime),
    ("小书本", SubjectType::Book),
    ("三刺螈", SubjectType::Real),
];

pub enum UIEvent {
    Key(termion::event::Key),
    Mouse(termion::event::MouseEvent),
}

#[derive(Clone)]
pub enum PendingUIEvent {
    Click(u16, u16, termion::event::MouseButton),
    ScrollIntoView(usize),
}

pub struct UIState {
    pub(crate) tab: usize,
    pub(crate) filters: [bool; SELECTS.len()],
    pub(crate) scroll: u16,
    pub(crate) focus: Option<usize>,
    pub(crate) focus_limit: usize,

    pub(crate) pending: Option<PendingUIEvent>,
}

impl Default for UIState {
    fn default() -> UIState {
        UIState {
            tab: 0,
            filters: [true; SELECTS.len()],

            scroll: 0,
            focus: None,
            focus_limit: 0,

            pending: None,
        }
    }
}

impl UIState {
    pub fn rotate_tab(&mut self) {
        if self.tab != TABS.len() - 1 {
            self.tab += 1;
        } else {
            self.tab = 0;
        }
    }

    pub fn select_tab(&mut self, mut tab: usize) {
        if tab >= TABS.len() {
            tab = TABS.len() - 1;
        }

        self.tab = tab;
    }

    pub fn set_focus_limit(&mut self, mf: usize) {
        self.focus_limit = mf;
        if let Some(f) = self.focus {
            if f >= mf {
                if mf == 0 {
                    self.focus = None;
                } else {
                    self.focus = Some(mf - 1);
                }
            }
        }
    }

    pub fn toggle_filter(&mut self, index: usize, entries: &Option<Vec<CollectionEntry>>) {
        // Get original index of the filter
        let original = self
            .focus
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

        self.focus = new_focus;
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
                    for (i, (_, t)) in SELECTS.iter().enumerate() {
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

        match ev {
            UIEvent::Key(Key::Down) if self.tab == 0 => match self.focus {
                None => {
                    self.focus = Some(0);
                    self.pending = Some(PendingUIEvent::ScrollIntoView(0));
                }
                Some(f) => {
                    if f + 1 < self.focus_limit {
                        self.focus = Some(f + 1);
                        self.pending = Some(PendingUIEvent::ScrollIntoView(f + 1));
                    }
                }
            },
            UIEvent::Key(Key::Up) if self.tab == 0 => {
                if let Some(f) = self.focus {
                    if f > 0 {
                        self.focus = Some(f - 1);
                        self.pending = Some(PendingUIEvent::ScrollIntoView(f - 1));
                    }
                }
            }
            UIEvent::Key(Key::Char('+')) if self.focus.is_some() => {
                let focus = self.focus.unwrap();
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
            UIEvent::Key(Key::Char('-')) if self.focus.is_some() => {
                let focus = self.focus.unwrap();
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
            UIEvent::Key(Key::Char('\t')) => {
                self.rotate_tab();
            }
            UIEvent::Mouse(m) => match m {
                MouseEvent::Press(btn, x, y) => {
                    self.pending = Some(PendingUIEvent::Click(x - 1, y - 1, btn))
                }
                MouseEvent::Hold(x, y) => {
                    self.pending = Some(PendingUIEvent::Click(
                        x - 1,
                        y - 1,
                        termion::event::MouseButton::Left,
                    ))
                }
                _ => {}
            },

            _ => {}
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

    pub fn set_scroll(&mut self, s: u16) {
        self.scroll = s;
    }

    pub fn scroll_delta(&mut self, delta: i16) {
        let new_scroll = self.scroll as i16 + delta;
        self.scroll = if new_scroll < 0 { 0 } else { new_scroll as u16 };
    }

    pub fn set_focus(&mut self, f: Option<usize>) {
        self.focus = f;
    }
}
