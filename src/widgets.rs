use bgmtv::client::{CollectionEntry, SubjectType, SubjectSmall};
use termion::event::MouseButton;
use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::style::{Color, Style};
use tui::symbols;
use tui::widgets::Widget;
use tui::widgets::{Block, Borders};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use crate::SubjectTypeExt;
use crate::state::ScrollState;

pub trait DynHeight: Widget {
    fn height(&self, width: u16) -> u16;
}

pub trait Intercept<Event> {
    fn intercept(&mut self, x: u16, y: u16, btn: MouseButton) -> Option<Event>;

    // Set the viewport for intercepting event
    fn set_bound(&mut self, _area: Rect) {}

    // Normalize internal state related to the bound, such as maximum value of scroll
    fn cap_bound(&mut self) {}
}

pub enum ScrollEvent {
    ScrollTo(u16),
    ScrollUp,
    ScrollDown,
    Sub(usize),
}

pub struct Scroll<'a> {
    content: Vec<&'a mut DynHeight>,
    bound: Rect,
    scroll: &'a mut ScrollState,
}

impl<'a> Scroll<'a> {
    pub fn with(scroll: &'a mut ScrollState) -> Self {
        Self {
            content: Vec::new(),
            bound: Rect::default(),
            scroll,
        }
    }

    pub fn inner_height(&self, width: u16) -> u16 {
        if width == 0 {
            return 0;
        }
        self.content.iter().fold(0, |acc, e| acc + e.height(width))
    }

    pub fn push(&mut self, comp: &'a mut DynHeight) {
        self.content.push(comp);
    }

    pub fn scroll_into_view(&mut self, index: usize) {
        let index = if index >= self.content.len() {
            self.content.len() - 1
        } else {
            index
        };

        let mut start = 0;
        for i in 0..index {
            start += self.content[i].height(self.bound.width-1);
        }

        let end = start + self.content[index].height(self.bound.width-1);

        let new_offset = if start < self.scroll.get() {
            start
        } else if end > self.scroll.get() + self.bound.height {
            end - self.bound.height
        } else {
            self.scroll.get()
        };

        self.scroll.set(new_offset);
    }
}

impl<'a> Widget for Scroll<'a> {
    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height < 1 {
            return;
        }

        let w = area.width - 1;

        if w < 2 {
            return;
        }

        let h = self.inner_height(w);
        let scroll = self.scroll.get();

        let mut dy = 0;
        for comp in self.content.iter_mut() {
            let height = comp.height(w);
            let width = w;
            let rect = Rect::new(0, 0, width, height);

            let mut subbuf = Buffer::empty(rect);
            comp.draw(rect, &mut subbuf);

            for iy in 0..height {
                if iy + dy < scroll {
                    continue;
                }

                let y = iy + dy - scroll;

                if y >= area.height {
                    break;
                }

                for x in 0..width {
                    *buf.get_mut(area.x + x, area.y + y) = subbuf.get(x, iy).clone();
                }
            }

            dy += height;
        }

        // Draw scroller
        if h > area.height {
            let vacant = area.height - 2;
            let pos = if self.scroll.get() == 0 {
                0
            } else if self.scroll.get() >= h - area.height {
                area.height - 2
            } else {
                let progress = (self.scroll.get() - 1) as usize;
                (progress * vacant as usize / (h - area.height) as usize) as u16 + 1
            };

            for y in 0..area.height {
                if y >= pos && y < pos + 2 {
                    buf.set_string(
                        area.x + area.width - 1,
                        area.y + y,
                        symbols::block::FULL,
                        Style::default(),
                    );
                } else {
                    buf.set_string(
                        area.x + area.width - 1,
                        area.y + y,
                        symbols::line::VERTICAL,
                        Style::default(),
                    );
                }
            }
        }
    }
}

impl<'a> Intercept<ScrollEvent> for Scroll<'a> {
    fn intercept(&mut self, x: u16, y: u16, btn: MouseButton) -> Option<ScrollEvent> {
        match btn {
            MouseButton::WheelUp => return Some(ScrollEvent::ScrollUp),
            MouseButton::WheelDown => return Some(ScrollEvent::ScrollDown),
            _ => {}
        }

        let h = self.inner_height(self.bound.width-1);

        if x == self.bound.x + self.bound.width - 1 {
            // Scrollbar
            if h > self.bound.height {
                let pos = y - self.bound.y;

                let scroll = if pos == 0 {
                    0
                } else if pos >= self.bound.height - 1 {
                    h - self.bound.height
                } else {
                    pos * (h - self.bound.height) / (self.bound.height - 2)
                };

                return Some(ScrollEvent::ScrollTo(scroll));
            }
        } else if x < self.bound.x + self.bound.width - 1 {
            // Is children
            let mut y = y - self.bound.y + self.scroll.get();

            for i in 0..self.content.len() {
                let h = self.content[i].height(self.bound.width-1);
                if h > y {
                    return Some(ScrollEvent::Sub(i));
                }

                y -= h;
            }
        }

        None
    }

    fn set_bound(&mut self, area: Rect) {
        self.bound = area;
    }

    fn cap_bound(&mut self) {
        let area = &self.bound;

        let new_height = self.inner_height(area.width-1);
        if new_height <= area.height {
            self.scroll.set(0);
        } else if new_height <= area.height + self.scroll.get() {
            self.scroll.set(new_height - area.height);
        }
    }
}

pub struct CJKText<'a> {
    content: Vec<(&'a str, Style)>,
}

impl<'a> CJKText<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { content: [(text, Style::default())].to_vec() }
    }

    pub fn raw<T: Into<Vec<(&'a str, Style)>>>(content: T) -> Self {
        Self { content: content.into() }
    }

    pub fn oneline_min_width(&self) -> u16 {
        let mut result = 0;
        for (t, _) in self.content.iter() {
            result += t.width() as u16;
        }

        result
    }

    pub fn set_style(&mut self, style: Style) {
        for (_, s) in self.content.iter_mut() {
            *s = style.clone();
        }
    }
}

impl<'a> Widget for CJKText<'a> {
    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        // Draw title
        let mut dy = 0;
        let mut dx = 0;

        for (text, style) in self.content.iter() {
            let tokens = text.graphemes(true);

            let mut last_present = true;

            for token in tokens {
                let newlines = token.chars().filter(|e| e == &'\n').count() as u16;
                if newlines > 0 {
                    if dx == 0 && last_present {
                        dy += newlines - 1;
                    } else {
                        dy += newlines;
                    }
                    dx = 0;

                    // This token only have invisible characters.
                    last_present = false;
                    continue;
                }

                last_present = true;

                let token_width = token.width() as u16;
                if token_width + dx > area.width {
                    dx = 0;
                    dy += 1;
                }

                if dy >= area.height {
                    return
                }

                buf.get_mut(dx + area.x, dy + area.y)
                    .set_symbol(token)
                    .set_style(style.clone());
                for i in 1..token_width {
                    buf.get_mut(dx + area.x + i, dy + area.y)
                        .set_symbol("")
                        .set_style(style.clone());
                }
                dx += token_width;
            }
        }
    }
}

impl<'a> DynHeight for CJKText<'a> {
    fn height(&self, width: u16) -> u16 {
        let mut result = 1;
        let mut acc = 0;
        for (text, _) in self.content.iter() {
            let tokens = text.graphemes(true);

            let mut last_present = true;

            for token in tokens {
                let newlines = token.chars().filter(|e| e == &'\n').count() as u16;
                if newlines > 0 {
                    if acc == 0 && last_present {
                        result += newlines - 1;
                    } else {
                        result += newlines;
                    }
                    acc = 0;

                    // This token only have invisible characters.
                    last_present = false;
                    continue;
                }

                last_present = true;

                let token_width = token.width() as u16;
                if token_width + acc > width {
                    acc = token_width;
                    result += 1;
                } else {
                    acc += token_width;
                }
            }
        }

        result
    }
}

pub enum ViewingEntryEvent {
    Click,
}

pub struct ViewingEntry<'a> {
    subject: &'a SubjectSmall,
    coll: Option<&'a CollectionEntry>,
    selected: bool,
}

impl<'a> ViewingEntry<'a> {
    pub fn progress(&self) -> Option<ViewProgress> {
        self.coll.map(|coll| {
            match self.subject.subject_type {
                SubjectType::Book => ViewProgress::new(
                    self.subject.vols_count,
                    coll.vol_status,
                ),
                _ => ViewProgress::new(
                    self.subject.eps_count,
                    coll.ep_status,
                ),
            }
        })
    }

    pub fn apply_text<R, F>(&'a self, cb: F) -> R 
        where for<'b> F: FnOnce(CJKText<'b>) -> R {
            let id = self.subject.id.to_string();

            let text = CJKText::raw([
                (self.subject.subject_type.disp(), Style::default().fg(Color::Blue)),
                (" ", Style::default()),
                (&id, Style::default()),
                ("\n\n", Style::default()),
                (self.subject.name.as_str(), Style::default().fg(Color::Yellow)),
                ("\n", Style::default()),
                (self.subject.name_cn.as_str(), Style::default().fg(Color::White)),
            ].to_vec());

            cb(text)
        }

    pub fn with_coll(ent: &'a CollectionEntry) -> Self {
        Self {
            subject: &ent.subject,
            coll: Some(ent),
            selected: false,
        }
    }

    pub fn with_subject(sub: &'a SubjectSmall) -> Self {
        Self {
            subject: sub,
            coll: None,
            selected: false,
        }
    }

    pub fn select(&mut self, s: bool) {
        self.selected = s;
    }
}

impl<'a> Widget for ViewingEntry<'a> {
    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width <= 2 {
            return;
        }

        let bs = if self.selected {
            Style::default().fg(Color::Green)
        } else {
            Style::default()
        };

        let mut b = Block::default().borders(Borders::ALL).border_style(bs);

        b.draw(area, buf);
        let inner = b.inner(area);

        let occupied_height = self.apply_text(|mut text| {
            text.draw(inner, buf);
            text.height(inner.width)
        }) + 1;

        if let Some(ref mut progress) = self.progress() {
            let new_area = Rect::new(
                inner.x,
                inner.y + occupied_height,
                inner.width,
                inner.height - occupied_height,
            );
            progress.draw(new_area, buf);
        }
    }
}

impl<'a> DynHeight for ViewingEntry<'a> {
    fn height(&self, width: u16) -> u16 {
        if width <= 2 {
            return 0
        }

        2 + self.apply_text(|t| t.height(width - 2))
            + self
                .progress()
                .as_ref()
                .map(|p| p.height(width - 2) + 1)
                .unwrap_or(0)
    }
}

impl<'a> Intercept<ViewingEntryEvent> for ViewingEntry<'a> {
    fn intercept(&mut self, _: u16, _: u16, _: MouseButton) -> Option<ViewingEntryEvent> {
        Some(ViewingEntryEvent::Click)
    }
}

pub enum TabberEvent {
    Select(usize),
    Close(usize),
    ScrollLeft,
    ScrollRight,
}

pub struct Tabber<'a> {
    tabs: &'a [&'a str],
    selected: Option<usize>,

    bound: Rect,
    scroll: &'a mut ScrollState,
}

impl<'a> Tabber<'a> {
    pub fn with(tabs: &'a [&'a str], scroll: &'a mut ScrollState) -> Self {
        Self {
            tabs,
            selected: None,
            bound: Rect::default(),
            scroll,
        }
    }

    pub fn select(mut self, index: usize) -> Self {
        self.selected = Some(index);
        self
    }

    pub fn inner_width(&self) -> u16 {
        self.tabs.iter().fold(0, |acc, x| acc + CJKText::new(x).oneline_min_width() + 2)
    }

    pub fn scroll_into_view(&mut self, index: usize) {
        let index = if index >= self.tabs.len() {
            self.tabs.len() - 1
        } else {
            index
        };

        let mut start = 0;
        for i in 0..index {
            start += CJKText::new(self.tabs[i]).oneline_min_width() + 2;
        }

        let end = start + CJKText::new(self.tabs[index]).oneline_min_width() + 2;

        let new_offset = if start < self.scroll.get() {
            start
        } else if end > self.scroll.get() + self.bound.width {
            end - self.bound.width
        } else {
            self.scroll.get()
        };

        self.scroll.set(new_offset);
    }
}

impl<'a> Widget for Tabber<'a> {
    fn draw(&mut self, viewport: Rect, buf: &mut Buffer) {
        let mut dx = 1;
        let scroll = self.scroll.get();
        eprintln!("{}", scroll);

        for (i, tab) in self.tabs.iter().enumerate() {
            let mut text = CJKText::new(tab);

            if Some(i) == self.selected {
                text.set_style(Style::default().fg(Color::Green));
            }

            let width = text.oneline_min_width();

            if viewport.width + scroll <= dx { // Already overflow
                break;
            }

            let width = std::cmp::min(width, viewport.width + scroll - dx);

            let area = Rect::new(0, 0, width, viewport.height);
            let mut subbuf = Buffer::empty(area);
            text.draw(area, &mut subbuf);

            // We cannot overflow the viewport here, because width is bounded
            for y in 0..viewport.height {

                let mut is_start = true;

                for x in 0..width {
                    if x + dx < scroll {
                        continue;
                    }

                    let cell = subbuf.get(x, y);
                    let target = buf.get_mut(x + dx + viewport.x - scroll, y + viewport.y);
                    *target = subbuf.get(x, y).clone();

                    // When doing horizontal scroll, we may break large unicode graphemes
                    if is_start && cell.symbol == "" {
                        target.set_symbol(" ");
                    } else {
                        is_start = false;
                    }

                }
            }

            dx += width + 2;
        }
    }
}

impl<'a> Intercept<TabberEvent> for Tabber<'a> {
    fn intercept(&mut self, x: u16, _: u16, btn: MouseButton) -> Option<TabberEvent> {
        match btn {
            MouseButton::WheelUp => return Some(TabberEvent::ScrollLeft),
            MouseButton::WheelDown => return Some(TabberEvent::ScrollRight),
            _ => {}
        }

        let dx = x - self.bound.x + self.scroll.get();
        let mut counter = 0;

        for (i, tab) in self.tabs.iter().enumerate() {
            let text = CJKText::new(tab);

            let width = text.oneline_min_width();
            counter += width + 2;

            if counter > dx {
                match btn {
                    MouseButton::Left => {
                        return Some(TabberEvent::Select(i));
                    },
                    MouseButton::Middle => {
                        return Some(TabberEvent::Close(i));
                    }
                    _ => {}
                }
            }
        }

        None
    }

    fn set_bound(&mut self, area: Rect) {
        self.bound = area;
    }

    fn cap_bound(&mut self) {
        let area = &self.bound;

        let tot_width = self.inner_width();
        if tot_width <= area.width {
            self.scroll.set(0);
        } else if tot_width <= area.width + self.scroll.get() {
            self.scroll.set(tot_width - area.width);
        }
    }
}

pub enum FilterListEvent {
    Toggle(usize),
}

pub struct FilterList<'a> {
    tabs: &'a [&'a str],
    state: &'a [bool],
    count: Option<&'a [usize]>,

    bound: Rect,
}

impl<'a> FilterList<'a> {
    pub fn with(tabs: &'a [&'a str], state: &'a [bool]) -> Self {
        Self {
            tabs,
            state,
            bound: Rect::default(),
            count: None,
        }
    }

    pub fn counting(mut self, c: &'a [usize]) -> Self {
        self.count = Some(c);
        self
    }
}

const VACANT_UNICODE: &str = "☐";
const SELECTED_UNICODE: &str = "✓";

impl<'a> Widget for FilterList<'a> {
    fn draw(&mut self, viewport: Rect, buf: &mut Buffer) {
        let mut dy = 0;
        for (i, tab) in self.tabs.iter().enumerate() {
            let mut symbol = if Some(&true) == self.state.get(i) {
                CJKText::new(SELECTED_UNICODE)
            } else {
                CJKText::new(VACANT_UNICODE)
            };

            symbol.set_style(Style::default().fg(Color::Red));
            symbol.draw(Rect::new(viewport.x, viewport.y + dy, 2, 1), buf);

            let width = viewport.width - 2;
            let text_style = if Some(&true) == self.state.get(i) {
                Style::default().fg(Color::White)
            } else {
                Style::default()
            };

            let count = self.count.and_then(|count| count.get(i)).map(|count| format!("({})", count));
            let mut text = if let Some(ref count) = count {
                CJKText::raw([
                    (*tab, text_style),
                    (" ", Style::default()),
                    (count, Style::default().fg(Color::Yellow)),
                ].to_vec())
            } else {
                let mut t = CJKText::new(tab);
                t.set_style(text_style);
                t
            };
            let height = text.height(width);

            let area = Rect::new(viewport.x + 2, viewport.y + dy, width, height);
            text.draw(area, buf);

            dy += height;
        }
    }
}

impl<'a> Intercept<FilterListEvent> for FilterList<'a> {
    fn intercept(&mut self, _x: u16, y: u16, _: MouseButton) -> Option<FilterListEvent> {
        let dy = y - self.bound.y;
        let mut counter = 0;
        for (i, tab) in self.tabs.iter().enumerate() {
            let width = self.bound.width - 2;
            let count = self.count.and_then(|count| count.get(i)).map(|count| format!("({})", count));
            let text = if let Some(ref count) = count {
                CJKText::raw([
                    (*tab, Style::default()),
                    (" ", Style::default()),
                    (count, Style::default()),
                ].to_vec())
            } else {
                CJKText::new(tab)
            };
            let height = text.height(width);
            counter += height;

            if counter > dy {
                return Some(FilterListEvent::Toggle(i));
            }
        }

        None
    }

    fn set_bound(&mut self, area: Rect) {
        self.bound = area;
    }
}

pub struct ViewProgress {
    total: Option<u64>,
    current: u64,
}

impl ViewProgress {
    fn new(total: Option<u64>, current: u64) -> Self {
        Self { total, current }
    }

    fn text_hint(&self) -> String {
        match self.total {
            Some(total) => 
                format!("{} / {}", self.current, total),
            None =>
                format!("{} / ?", self.current),
        }
    }
}

const SHADE: &str = "▒";

impl Widget for ViewProgress {
    fn draw(&mut self, viewport: Rect, buf: &mut Buffer) {
        // Write digits
        let text = self.text_hint();
        let mut text_widget = CJKText::new(&text);
        text_widget.draw(viewport, buf);

        let text_height = text_widget.height(viewport.width);

        // Draw blocks
        for i in 0..self.total.unwrap_or(self.current + 1) as u16 {
            let dy = i / viewport.width;
            let dx = i % viewport.width;

            if dy + text_height >= viewport.height {
                break;
            }

            let (style, symbol) = if (i as u64) < self.current {
                (Style::default().fg(Color::White), symbols::block::FULL)
            } else {
                (Style::default(), SHADE)
            };

            buf.get_mut(viewport.x + dx, viewport.y + text_height + dy)
                .set_symbol(symbol)
                .set_style(style);
        }
    }
}

impl DynHeight for ViewProgress {
    fn height(&self, width: u16) -> u16 {
        if width == 0 {
            0
        } else {
            let text = self.text_hint();
            let text_widget = CJKText::new(&text);
            text_widget.height(width) + (self.total.unwrap_or(self.current + 1) as u16 + width - 1) / width
        }
    }
}

pub struct SingleCell<'a> {
    symbol: &'a str,
}

impl<'a> SingleCell<'a> {
    pub fn new(symbol: &'a str) -> Self {
        Self { symbol }
    }
}

impl<'a> Widget for SingleCell<'a> {
    fn draw(&mut self, viewport: Rect, buf: &mut Buffer) {
        buf.get_mut(viewport.x, viewport.y).set_symbol(self.symbol);
    }
}
