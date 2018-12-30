use bgmtv::client::CollectionEntry;
use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::style::{Style, Color};
use tui::widgets::Widget;
use tui::widgets::{Block, Borders};
use tui::symbols;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub trait DynHeight: Widget {
    fn height(&self, width: u16) -> u16;
}

pub trait Intercept<Event> {
    fn intercept(&mut self, x: u16, y: u16) -> Option<Event>;
}

pub enum ScrollEvent {
    ScrollTo(u16),
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

    pub fn set_bound(&mut self, area: Rect) {
        self.bound = area;

        let new_height = self.inner_height(area.width);
        if new_height <= area.height {
            eprintln!("SMALLER");
            self.offset = 0;
        } else if new_height <= area.height + self.offset {
            self.offset = new_height - area.height;
        }
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
        } else { index };

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
                    buf.set_string(area.x + area.width - 1, area.y + y, symbols::block::FULL, Style::default());
                } else {
                    buf.set_string(area.x + area.width - 1, area.y + y, symbols::line::VERTICAL, Style::default());
                }
            }
        }
    }
}

impl<'a> Intercept<ScrollEvent> for Scroll<'a> {
    fn intercept(&mut self, x: u16, y: u16) -> Option<ScrollEvent> {
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
        } else if x < self.bound.x + self.bound.width - 1{
            // Is children
            let mut y = y - self.bound.y + self.offset;

            for i in 0..self.content.len() {
                let h = self.content[i].height(self.bound.width);
                if h > y {
                    return Some(ScrollEvent::Sub(i));
                }

                y -= h;
            }

            return Some(ScrollEvent::Sub(self.content.len()-1));
        }

        None
    }
}

pub struct CJKText<'a> {
    text: &'a str,
}

impl<'a> CJKText<'a> {
    fn new(text: &'a str) -> Self {
        Self { text }
    }
}

impl<'a> Widget for CJKText<'a> {
    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        // Draw title
        let mut dy = 0;
        let mut dx = 0;

        let tokens = UnicodeSegmentation::graphemes(self.text, true);

        for token in tokens {
            let token_width = UnicodeWidthStr::width_cjk(token) as u16;
            if token_width + dx > area.width {
                dx = 0;
                dy += 1;
            }

            buf.get_mut(dx + area.x, dy + area.y).set_symbol(token);
            for i in 1..token_width {
                buf.get_mut(dx + area.x + i, dy + area.y).set_symbol("");
            }
            dx += token_width;
        }
    }
}

impl<'a> DynHeight for CJKText<'a> {
    fn height(&self, width: u16) -> u16 {
        let tokens = UnicodeSegmentation::graphemes(self.text, true);

        let mut acc = 0;
        let mut result = 1;
        for token in tokens {
            let token_width = UnicodeWidthStr::width_cjk(token) as u16;
            if token_width + acc > width {
                acc = token_width;
                result += 1;
            } else {
                acc += token_width;
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
    text: CJKText<'a>,

    selected: bool,
}

impl<'a> ViewingEntry<'a> {
    pub fn new(ent: &'a CollectionEntry) -> Self {
        let text = CJKText::new(ent.subject.name.as_str());
        Self { coll: ent, selected: false, text }
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
        self.text.draw(inner, buf);
    }
}

impl<'a> DynHeight for ViewingEntry<'a> {
    fn height(&self, width: u16) -> u16 {
        self.text.height(width - 2) + 2
    }
}

impl<'a> Intercept<ViewingEntryEvent> for ViewingEntry<'a> {
    fn intercept(&mut self, _: u16, _: u16) -> Option<ViewingEntryEvent> {
        Some(ViewingEntryEvent::Click)
    }
}
