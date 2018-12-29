use bgmtv::client::CollectionEntry;
use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::style::Style;
use tui::widgets::Widget;
use tui::widgets::{Block, Borders};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use std::any::Any;

pub trait DynHeight: Widget {
    fn height(&self, width: u16) -> u16;
}

pub trait Intercept {
    fn intercept(&mut self, x: u16, y: u16);
}

pub enum ScrollEvent {
    ScrollTo(u16),
}

pub struct Scroll<'a> {
    content: Vec<&'a mut DynHeight>,
    offset: u16,

    bound: Rect,
    listener: Option<Box<'a + FnMut(ScrollEvent) -> ()>>,
}

impl<'a> Default for Scroll<'a> {
    fn default() -> Self {
        Self {
            content: Vec::new(),
            offset: 0,
            bound: Rect::default(),
            listener: None,
        }
    }
}

impl<'a> Scroll<'a> {
    fn inner_height(&self, width: u16) -> u16 {
        self.content.iter().fold(0, |acc, e| acc + e.height(width))
    }

    pub fn scroll(self, s: u16) -> Self {
        Self {
            content: self.content,
            bound: self.bound,
            offset: s,
            listener: self.listener,
        }
    }

    pub fn listen<T: 'a + FnMut(ScrollEvent) -> ()>(self, f: T) -> Self {
        Self {
            content: self.content,
            bound: self.bound,
            offset: self.offset,
            listener: Some(Box::new(f)),
        }
    }

    fn set_bound(&mut self, area: Rect) {
        self.bound = area;

        let original_offset = self.offset;

        let new_height = self.inner_height(area.width);
        if new_height <= area.height {
            self.offset = 0;
        } else if new_height <= area.height + self.offset {
            self.offset = new_height - area.height;
        }

        if original_offset != self.offset {
            if let Some(ref mut f) = self.listener {
                f(ScrollEvent::ScrollTo(self.offset));
            }
        }
    }

    pub fn push(&mut self, comp: &'a mut DynHeight) {
        self.content.push(comp);
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
                    buf.get_mut(area.x + x, area.y + y).set_symbol(&cell.symbol);
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
                    buf.set_string(area.x + area.width - 1, area.y + y, "=", Style::default());
                } else {
                    buf.set_string(area.x + area.width - 1, area.y + y, "|", Style::default());
                }
            }
        }
    }
}

impl<'a> Intercept for Scroll<'a> {
    fn intercept(&mut self, x: u16, y: u16) {
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

                if let Some(ref mut listener) = self.listener {
                    listener(ScrollEvent::ScrollTo(scroll));
                }
            }
        }
    }
}

pub struct ViewingEntry<'a> {
    coll: &'a CollectionEntry,
}

impl<'a> ViewingEntry<'a> {
    pub fn new(ent: &'a CollectionEntry) -> Self {
        Self { coll: ent }
    }

    fn title_height(&self, inner_width: u16) -> u16 {
        let tokens = UnicodeSegmentation::graphemes(self.coll.subject.name.as_str(), true);

        let mut acc = 0;
        let mut result = 1;
        for token in tokens {
            let token_width = UnicodeWidthStr::width_cjk(token) as u16;
            if token_width + acc > inner_width {
                acc = token_width;
                result += 1;
            } else {
                acc += token_width;
            }
        }

        result
    }
}

impl<'a> Widget for ViewingEntry<'a> {
    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        let mut b = Block::default().borders(Borders::ALL);
        b.draw(area, buf);
        let inner = b.inner(area);

        // Draw title
        let mut dy = 0;
        let mut dx = 0;

        let name = self.coll.subject.name.as_str();

        let tokens = UnicodeSegmentation::graphemes(name, true);

        for token in tokens {
            let token_width = UnicodeWidthStr::width_cjk(token) as u16;
            if token_width + dx > inner.width {
                dx = 0;
                dy += 1;
            }

            buf.get_mut(dx + inner.x, dy + inner.y).set_symbol(token);
            for i in 1..token_width {
                buf.get_mut(dx + inner.x + i, dy + inner.y).set_symbol("");
            }
            dx += token_width;
        }
    }
}

impl<'a> DynHeight for ViewingEntry<'a> {
    fn height(&self, width: u16) -> u16 {
        self.title_height(width - 2) + 2
    }
}
