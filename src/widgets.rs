use bgmtv::client::{CollectionEntry, SubjectType};
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

pub trait DynHeight: Widget {
    fn height(&self, width: u16) -> u16;
}

pub trait Intercept<Event> {
    fn intercept(&mut self, x: u16, y: u16, btn: MouseButton) -> Option<Event>;
    fn set_bound(&mut self, _area: Rect) {}
}

pub enum ScrollEvent {
    ScrollTo(u16),
    ScrollUp,
    ScrollDown,
    Sub(usize),
}

pub struct Scroll<'a> {
    content: Vec<&'a mut DynHeight>,
    offset: u16,

    bound: Rect,
}

impl<'a> Default for Scroll<'a> {
    fn default() -> Self {
        Self {
            content: Vec::new(),
            offset: 0,
            bound: Rect::default(),
        }
    }
}

impl<'a> Scroll<'a> {
    fn inner_height(&self, width: u16) -> u16 {
        self.content.iter().fold(0, |acc, e| acc + e.height(width))
    }

    pub fn scroll(mut self, s: u16) -> Self {
        self.offset = s;
        self
    }

    pub fn get_scroll(&self) -> u16 {
        self.offset
    }

    pub fn push(&mut self, comp: &'a mut DynHeight) {
        self.content.push(comp);
    }

    pub fn scroll_into_view(&mut self, index: usize) {
        let index = if index > self.content.len() {
            self.content.len()
        } else {
            index
        };

        let mut start = 0;
        for i in 0..index {
            start += self.content[i].height(self.bound.width);
        }

        let end = start + self.content[index].height(self.bound.width);

        let new_offset = if start < self.offset {
            start
        } else if end > self.offset + self.bound.height {
            end - self.bound.height
        } else {
            self.offset
        };

        self.offset = new_offset;
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

        self.set_bound(area);

        let h = self.inner_height(w);
        let scroll = self.offset;

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
                    let cell = subbuf.get(x, iy);
                    std::mem::replace(buf.get_mut(area.x + x, area.y + y), cell.clone());
                }
            }

            dy += height;
        }

        // Draw scroller
        if h > area.height {
            let vacant = area.height - 2;
            let pos = if self.offset == 0 {
                0
            } else if self.offset >= h - area.height {
                area.height - 2
            } else {
                let progress = (self.offset - 1) as usize;
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

        let h = self.inner_height(self.bound.width);

        if x == self.bound.x + self.bound.width - 1 {
            // Scrollbar
            if h > self.bound.height {
                let pos = y - self.bound.y;

                let scroll = if pos == 0 {
                    0
                } else if pos >= self.bound.height - 1 {
                    h - self.bound.height
                } else {
                    (pos - 1) * (h - self.bound.height) / (self.bound.height - 2)
                };

                return Some(ScrollEvent::ScrollTo(scroll));
            }
        } else if x < self.bound.x + self.bound.width - 1 {
            // Is children
            let mut y = y - self.bound.y + self.offset;

            for i in 0..self.content.len() {
                let h = self.content[i].height(self.bound.width);
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

        let new_height = self.inner_height(area.width);
        if new_height <= area.height {
            self.offset = 0;
        } else if new_height <= area.height + self.offset {
            self.offset = new_height - area.height;
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
            result += t.width_cjk() as u16;
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

                let token_width = token.width_cjk() as u16;
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

                let token_width = token.width_cjk() as u16;
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
    coll: &'a CollectionEntry,
    selected: bool,
}

impl<'a> ViewingEntry<'a> {
    pub fn progress(&self) -> Option<ViewProgress> {
        match self.coll.subject.subject_type {
            SubjectType::Book => Some(ViewProgress::new(
                self.coll.subject.vols_count,
                self.coll.vol_status,
            )),
            _ => Some(ViewProgress::new(
                self.coll.subject.eps_count,
                self.coll.ep_status,
            )),
        }
    }

    pub fn apply_text<R, F>(&'a self, cb: F) -> R 
        where for<'b> F: FnOnce(CJKText<'b>) -> R {
            let id = self.coll.subject.id.to_string();

            let text = CJKText::raw([
                (self.coll.subject.subject_type.disp(), Style::default().fg(Color::Blue)),
                (" ", Style::default()),
                (&id, Style::default()),
                ("\n", Style::default()),
                (self.coll.subject.name.as_str(), Style::default().fg(Color::Yellow)),
                ("\n", Style::default()),
                (self.coll.subject.name_cn.as_str(), Style::default().fg(Color::White)),
            ].to_vec());

            cb(text)
        }

    pub fn new(ent: &'a CollectionEntry) -> Self {
        Self {
            coll: ent,
            selected: false,
        }
    }

    pub fn select(&mut self, s: bool) {
        self.selected = s;
    }
}

impl<'a> Widget for ViewingEntry<'a> {
    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
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
}

pub struct Tabber<'a> {
    tabs: &'a [&'a str],
    selected: Option<usize>,

    bound: Rect,
}

impl<'a> Tabber<'a> {
    pub fn with(tabs: &'a [&'a str]) -> Self {
        Self {
            tabs,
            selected: None,
            bound: Rect::default(),
        }
    }

    pub fn select(mut self, index: usize) -> Self {
        self.selected = Some(index);
        self
    }
}

impl<'a> Widget for Tabber<'a> {
    fn draw(&mut self, viewport: Rect, buf: &mut Buffer) {
        let mut dx = 1;

        for (i, tab) in self.tabs.iter().enumerate() {
            let mut text = CJKText::new(tab);

            if Some(i) == self.selected {
                text.set_style(Style::default().fg(Color::Green));
            }

            let width = text.oneline_min_width();
            let maxwidth = viewport.width - dx;

            let width = std::cmp::min(width, maxwidth);

            if width == 0 {
                break;
            }

            let area = Rect::new(viewport.x + dx, viewport.y, width, viewport.height);
            text.draw(area, buf);

            dx += width + 2;
        }
    }
}

impl<'a> Intercept<TabberEvent> for Tabber<'a> {
    fn intercept(&mut self, x: u16, _: u16, btn: MouseButton) -> Option<TabberEvent> {
        let dx = x - self.bound.x;
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
