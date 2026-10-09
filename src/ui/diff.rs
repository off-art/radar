//! Просмотр изменений (diff) и коммит.

use super::popup::popup_block;
use super::{clear, fit};
use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Окно просмотра: (всё окно, список файлов, область строк diff).
pub fn diff_layout(area: Rect) -> (Rect, Rect, Rect) {
    let popup = Rect::new(
        area.x + 2.min(area.width / 4),
        area.y + 1.min(area.height / 4),
        area.width.saturating_sub(4).max(20),
        area.height.saturating_sub(2).max(8),
    );
    let popup = Rect::new(popup.x, popup.y, popup.width.min(area.width), popup.height.min(area.height));
    let inner = Rect::new(popup.x + 1, popup.y + 1, popup.width.saturating_sub(2), popup.height.saturating_sub(2));
    let lw = (inner.width / 4).clamp(16, 36).min(inner.width.saturating_sub(10));
    let h = inner.height.saturating_sub(2);
    let list = Rect::new(inner.x, inner.y + 1, lw, h);
    let body = Rect::new(inner.x + lw + 1, inner.y + 1, inner.width.saturating_sub(lw + 1), h);
    (popup, list, body)
}

/// Индекс первого видимого файла в списке (выбранный всегда на экране).
pub fn diff_list_first(v: &crate::diff::View, rows: usize) -> usize {
    if rows == 0 || v.sel < rows {
        0
    } else {
        v.sel + 1 - rows
    }
}

/// Оставляет конец строки (имя файла важнее начала пути).
pub(super) fn fit_tail(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut used = 1; // под «…»
    for c in chars.iter().rev() {
        let cw = c.width().unwrap_or(0);
        if used + cw > w {
            break;
        }
        out.insert(0, *c);
        used += cw;
    }
    format!("…{out}")
}

pub(super) fn draw_diff(f: &mut Frame, app: &App, v: &crate::diff::View) {
    use crate::diff::Kind;
    let th = &app.theme;
    let (popup, list, body) = diff_layout(f.area());
    clear(f, popup, th);
    f.render_widget(popup_block(th, &format!("Изменения — {}", v.title)), popup);
    let inner = Rect::new(popup.x + 1, popup.y + 1, popup.width.saturating_sub(2), popup.height.saturating_sub(2));

    let (ta, tr) = v.total();
    let head = Line::from(vec![
        Span::styled(format!(" {} файл(ов) ", v.files.len()), Style::default().fg(th.dim)),
        Span::styled(format!("+{ta} "), Style::default().fg(th.green)),
        Span::styled(format!("−{tr}"), Style::default().fg(th.red)),
    ]);
    f.render_widget(Paragraph::new(head), Rect::new(inner.x, inner.y, inner.width, 1));

    let buf = f.buffer_mut();
    // разделитель между списком и diff
    for y in list.y..list.bottom() {
        if let Some(c) = buf.cell_mut((list.right(), y)) {
            c.set_symbol("│").set_style(Style::default().fg(th.line));
        }
    }
    let first = diff_list_first(v, list.height as usize);
    for (row, (i, file)) in v.files.iter().enumerate().skip(first).take(list.height as usize).enumerate() {
        let r = Rect::new(list.x, list.y + row as u16, list.width, 1);
        let active = i == v.sel;
        let base = if active { Style::default().bg(th.active_row_bg) } else { Style::default() };
        if active {
            buf.set_style(r, base);
        }
        let sc = match file.status {
            'A' | '?' => th.green,
            'D' => th.red,
            'R' => th.blue,
            _ => th.peach,
        };
        let stat = format!(" +{} −{}", file.added, file.removed);
        let name_w = (r.width as usize).saturating_sub(3 + stat.width());
        let line = Line::from(vec![
            Span::styled(format!(" {} ", file.status), base.fg(sc).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{:<w$}", fit_tail(&file.path, name_w), w = name_w),
                base.add_modifier(if active { Modifier::BOLD } else { Modifier::empty() }),
            ),
            Span::styled(stat, base.fg(th.dim)),
        ]);
        buf.set_line(r.x, r.y, &line, r.width);
    }

    // строки выбранного файла
    if let Some(file) = v.file() {
        let h = body.height as usize;
        let w = body.width as usize;
        for (row, l) in file.lines.iter().skip(v.scroll).take(h).enumerate() {
            let (prefix, style) = match l.kind {
                Kind::Add => ("+", Style::default().fg(th.green)),
                Kind::Del => ("-", Style::default().fg(th.red)),
                Kind::Hunk => ("", Style::default().fg(th.blue).add_modifier(Modifier::BOLD)),
                Kind::Meta => ("", Style::default().fg(th.dim).add_modifier(Modifier::ITALIC)),
                Kind::Ctx => (" ", Style::default().fg(th.subtext)),
            };
            let text = format!("{prefix}{}", l.text.replace('\t', "    "));
            buf.set_line(body.x, body.y + row as u16, &Line::from(Span::styled(fit(&text, w), style)), body.width);
        }
        // позиция в файле справа в шапке
        let total = file.lines.len();
        if total > h {
            let pos = format!(" {}–{} из {} ", v.scroll + 1, (v.scroll + h).min(total), total);
            let x = inner.right().saturating_sub(pos.width() as u16);
            buf.set_line(x, inner.y, &Line::from(Span::styled(pos, Style::default().fg(th.dim))), inner.width);
        }
    }

    let hint = Line::from(Span::styled(
        "j/k, колесо — строки · n/p, клик — файл · PgUp/PgDn · c — коммит · P — push · r — обновить · Esc — закрыть",
        Style::default().fg(th.dim),
    ));
    buf.set_line(inner.x + 1, inner.bottom().saturating_sub(1), &hint, inner.width.saturating_sub(2));
}
