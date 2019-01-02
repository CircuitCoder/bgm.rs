use crate::state::{UIState, Tab};
use crate::widgets::CJKText;
use tui::style::{Style,Modifier,Color};

pub struct HelpEntry(
    &'static [&'static str],
    &'static str,
    &'static Fn(&UIState) -> bool,
);

impl HelpEntry {
    pub fn pred(&self) -> &'static Fn(&UIState) -> bool {
        self.2
    }
}

impl<'a> Into<CJKText<'static>> for &'a HelpEntry {
    fn into(self) -> CJKText<'static> {
        let mut result = Vec::new();

        for i in 0..self.0.len() {
            if i != 0 {
                result.push((" / ", Style::default()));
            }
            result.push((self.0[i], Style::default().modifier(Modifier::Bold).fg(Color::Red)));
        }

        result.push((": ", Style::default()));
        result.push((self.1, Style::default()));

        CJKText::raw(result)
    }
}

fn is_subject(ui: &UIState) -> bool {
    ui.active_tab().is_subject()
}

fn is_collection(ui: &UIState) -> bool {
    ui.active_tab().is_collection()
}

fn is_search(ui: &UIState) -> bool {
    ui.active_tab().is_search()
}

fn is_search_result(ui: &UIState) -> bool {
    ui.active_tab().is_search_result()
}

pub const HELP_DATABASE: [HelpEntry; 32] = [
    // General
    HelpEntry(&["?", "h", ":help"], "康帮助", &|_| true),
    HelpEntry(&["K"], "向上滚动帮助", &|ui| ui.help),
    HelpEntry(&["J"], "向下滚动帮助", &|ui| ui.help),
    HelpEntry(&[":qa", "C-q"], "Rage quit", &|_| true),

    HelpEntry(&["R"], "刷新", &|ui| !is_search(ui)),

    // On primary tab
    HelpEntry(&["k", "Up"], "选择上一个", &|ui| is_collection(ui)),
    HelpEntry(&["j", "Down"], "选择下一个", &|ui| is_collection(ui)),
    HelpEntry(&["t<i>"], "切换第 i 个过滤选项", &|ui| is_collection(ui)),

    // When have focus
    HelpEntry(&["+"], "增加进度", &|ui| is_collection(ui) && ui.focus.get().is_some()),
    HelpEntry(&["-"], "减少进度", &|ui| is_collection(ui) && ui.focus.get().is_some()),
    HelpEntry(&["Enter"], "详情/编辑", &|ui| is_collection(ui) && ui.focus.get().is_some()),
    HelpEntry(&["Esc"], "取消选择", &|ui| is_collection(ui) && ui.focus.get().is_some() && !ui.command.present()),

    // When in subject page
    HelpEntry(&["s"], "修改收藏状态", &is_subject),
    HelpEntry(&["r"], "修改评分", &is_subject),
    HelpEntry(&["t"], "修改标签", &is_subject),
    HelpEntry(&["c"], "修改评论", &is_subject),
    HelpEntry(&["Esc"], "也可以关闭标签", &|ui| is_subject(ui) && !ui.command.present()),

    // When in search page
    HelpEntry(&["e", "Enter"], "修改搜索文字", &|ui| if let Tab::Search{ text } = ui.active_tab() { text == "" } else { false }),
    HelpEntry(&["e"], "修改搜索文字", &|ui| if let Tab::Search{ text } = ui.active_tab() { text != "" } else { false }),
    HelpEntry(&["Enter"], "搜索", &|ui| if let Tab::Search{ text } = ui.active_tab() { text != "" } else { false }),

    // In search result
    HelpEntry(&["n"], "下一页", &|ui| is_search_result(ui)),
    HelpEntry(&["N"], "上一页", &|ui| is_search_result(ui)),
    HelpEntry(&["k", "Up"], "选择上一个", &|ui| is_search_result(ui)),
    HelpEntry(&["j", "Down"], "选择下一个", &|ui| is_search_result(ui)),

    // Long command
    HelpEntry(&["Esc"], "取消命令", &|ui| ui.command.present()),

    // Tabs
    HelpEntry(&["gt", "Tab"], "下一个 Tab", &|_| true),
    HelpEntry(&["gT"], "上一个 Tab", &|_| true),
    HelpEntry(&["gg"], "滚动至顶", &|ui| !is_search(ui)),
    HelpEntry(&["G"], "滚动至底", &|ui| !is_search(ui)),

    HelpEntry(&[":tabe <coll|search>"], "打开格子/搜索 Tab", &|_| true),
    HelpEntry(&[":tabm <n>"], "移动 Tab", &|_| true),
    HelpEntry(&[":q"], "关闭 Tab", &|_| true),
];
