//! Отрисовка интерфейса (ratatui) и расчёт областей для кликов мыши.

use self::diff::draw_diff;
use self::form::{draw_form, draw_group_form};
use self::overlays::{draw_confirm, draw_help, draw_menu, draw_palette};
use self::panes::{draw_pane, draw_welcome};
use self::popup::draw_input_popup;
use self::settings::{draw_integrations, draw_log, draw_settings};
use self::sidebar::draw_sidebar;
use self::statusbar::draw_statusbar;
use crate::app::{App, Mode};
use crate::session::Session;
use crate::status::Status;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod diff;
mod form;
mod overlays;
mod panes;
mod popup;
mod settings;
mod sidebar;
mod statusbar;

pub use diff::{diff_layout, diff_list_first};
pub use form::{form_layout, group_hit};
pub use overlays::{confirm_hit, palette_hit};
pub use popup::input_hit;
pub use settings::{integrations_layout, log_layout, settings_layout, tab_rects};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn status_color(th: &Theme, s: &Session) -> Color {
    match s.status {
        Status::Starting => th.dim,
        Status::Working => th.blue,
        Status::Waiting => th.accent,
        Status::Idle => th.green,
        Status::Exited => {
            if s.exit_code.unwrap_or(0) == 0 {
                th.dim
            } else {
                th.red
            }
        }
    }
}

fn icon(s: &Session, ms: u128) -> &'static str {
    match s.status {
        Status::Working => SPINNER[(ms / 80) as usize % SPINNER.len()],
        Status::Waiting => {
            if (ms / 500).is_multiple_of(2) {
                "●"
            } else {
                "○"
            }
        }
        Status::Idle => "✓",
        Status::Starting => "◌",
        Status::Exited => "■",
    }
}

fn fmt_dur(secs: u64) -> String {
    crate::events::fmt_dur(secs)
}

fn status_time(s: &Session) -> String {
    match s.status {
        Status::Working => fmt_dur(s.work_started.unwrap_or(s.status_since).elapsed().as_secs()),
        Status::Waiting | Status::Idle => fmt_dur(s.status_since.elapsed().as_secs()),
        _ => String::new(),
    }
}

/// Обрезает строку по ширине с многоточием.
fn fit(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw + 1 > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out
}

fn push_fit(spans: &mut Vec<Span<'static>>, text: &str, style: Style, left: &mut usize) {
    if *left == 0 || text.is_empty() {
        return;
    }
    let t = fit(text, *left);
    *left = left.saturating_sub(t.width());
    spans.push(Span::styled(t, style));
}

/// Очищает область под всплывающим окном и закрашивает её фоном темы.
fn clear(f: &mut Frame, r: Rect, th: &Theme) {
    f.render_widget(Clear, r);
    if th.paint {
        f.buffer_mut().set_style(r, Style::default().bg(th.popup_bg).fg(th.text));
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let th = &app.theme;
    if th.paint {
        let area = f.area();
        f.buffer_mut().set_style(area, Style::default().bg(th.bg).fg(th.text));
    }
    draw_sidebar(f, app);
    if app.sessions.is_empty() {
        draw_welcome(f, app);
    }
    for p in &app.geo.panes {
        draw_pane(f, app, p);
    }
    draw_statusbar(f, app);
    match &app.mode {
        Mode::New(form) => draw_form(f, app, form),
        Mode::Rename(t) => draw_input_popup(f, "Переименовать агента", t, &app.theme),
        Mode::GroupForm(g) => draw_group_form(f, app, g),
        Mode::Confirm(c) => draw_confirm(f, app, c),
        Mode::Help => draw_help(f, app),
        Mode::Menu(m) => draw_menu(f, m, &app.theme),
        Mode::Palette(p) => draw_palette(f, p, &app.theme),
        Mode::Settings(sel) => draw_settings(f, app, *sel),
        Mode::Integrations(sel) => draw_integrations(f, app, *sel),
        Mode::Diff(v) => draw_diff(f, app, v),
        Mode::Log(sel) => draw_log(f, app, *sel),
        Mode::Commit(t) => draw_input_popup(f, "Сообщение коммита", t, &app.theme),
        _ => {}
    }
}
