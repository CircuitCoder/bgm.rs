use bgmtv::client::CollectionEntry;
use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::style::Style;
use tui::widgets::Widget;
use tui::widgets::{Block, Borders};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub trait DynHeight: Widget {
    fn height(&self, width: u16) -> u16;
}

pub struct Scroll<'a> {
    content: Vec<&'a mut DynHeight>,
    offset: u16,
}

impl<'a> Default for Scroll<'a> {
    fn default() -> Self {
        Self {
            content: Vec::new(),
            offset: 0,
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
            offset: s,
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

        let h = self.inner_height(w);

        let scroll = if h <= area.height {
            0
        } else {
            std::cmp::min(h - area.height, self.offset)
        };

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
