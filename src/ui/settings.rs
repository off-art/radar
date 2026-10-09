//! Окно настроек, интеграции и лента событий.

use super::clear;
use super::popup::{centered, popup_block};
use crate::app::App;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

/// Окно настроек и прямоугольники его строк (общее для отрисовки и мыши).
pub fn settings_layout(area: Rect, rows: usize) -> (Rect, Vec<Rect>) {
    let popup = centered(area, 64, rows as u16 + 6);
    let inner = Rect::new(popup.x + 1, popup.y + 1, popup.width.saturating_sub(2), popup.height.saturating_sub(2));
    let rects = (0..rows).map(|i| Rect::new(inner.x, inner.y + 1 + i as u16, inner.width, 1)).collect();
    (popup, rects)
}

pub(super) fn draw_settings(f: &mut Frame, app: &App, sel: usize) {
    let th = &app.theme;
    let rows = app.settings_rows();
    let (popup, rects) = settings_layout(f.area(), rows.len());
    clear(f, popup, th);
    f.render_widget(popup_block(th, "Настройки"), popup);
    draw_tabs(f, popup, 0, th);
    for (i, ((label, value), r)) in rows.iter().zip(&rects).enumerate() {
        let active = i == sel;
        let base = if active { Style::default().bg(th.active_row_bg) } else { Style::default() };
        let w = r.width as usize;
        let val = format!("‹ {value} ›");
        let gap = w.saturating_sub(label.width() + val.width() + 3);
        let line = Line::from(vec![
            Span::styled(if active { " ▸ " } else { "   " }, base.fg(th.accent)),
            Span::styled(label.clone(), base.add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
            Span::styled(" ".repeat(gap), base),
            Span::styled(val, base.fg(if active { th.accent } else { th.dim })),
        ]);
        f.render_widget(Paragraph::new(line).style(base), *r);
    }
    let hint = Rect::new(popup.x + 2, popup.bottom().saturating_sub(3), popup.width.saturating_sub(4), 2);
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "↑/↓ — строка · ←/→ или клик — изменить · Tab — раздел · Esc — закрыть",
                Style::default().fg(th.dim),
            )),
            Line::from(Span::styled(
                "Цветовая схема применяется сразу. Свои цвета: [theme.custom] в config.toml",
                Style::default().fg(th.dim),
            )),
        ]),
        hint,
    );
}

/// Прямоугольники вкладок «Настройки» / «Интеграции» в шапке окна настроек.
pub fn tab_rects(popup: Rect) -> [Rect; 2] {
    let y = popup.y + 1;
    let x = popup.x + 2;
    let w0 = "Настройки".width() as u16 + 2;
    let w1 = "Интеграции".width() as u16 + 2;
    [Rect::new(x, y, w0, 1), Rect::new(x + w0 + 1, y, w1, 1)]
}

pub(super) fn draw_tabs(f: &mut Frame, popup: Rect, active: usize, th: &Theme) {
    for (i, (r, name)) in tab_rects(popup).iter().zip(["Настройки", "Интеграции"]).enumerate() {
        let st = if i == active {
            Style::default().fg(th.accent).bg(th.active_row_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.dim)
        };
        f.render_widget(Paragraph::new(format!(" {name} ")).style(st), *r);
    }
}

pub fn integrations_layout(area: Rect, rows: usize) -> (Rect, Vec<Rect>) {
    let popup = centered(area, 72, rows as u16 + 9);
    let inner = Rect::new(popup.x + 1, popup.y + 1, popup.width.saturating_sub(2), popup.height.saturating_sub(2));
    let rects = (0..rows).map(|i| Rect::new(inner.x, inner.y + 3 + i as u16, inner.width, 1)).collect();
    (popup, rects)
}

pub fn log_layout(area: Rect, total: usize, sel: usize) -> (Rect, Vec<Rect>, usize) {
    let h = (total as u16).clamp(3, 20) + 4;
    let popup = centered(area, 90, h);
    let inner = Rect::new(popup.x + 1, popup.y + 1, popup.width.saturating_sub(2), popup.height.saturating_sub(2));
    let visible = inner.height.saturating_sub(2) as usize;
    let shown = visible.min(total);
    let first = if sel >= visible { sel + 1 - visible } else { 0 };
    let rows = (0..shown).map(|i| Rect::new(inner.x, inner.y + i as u16, inner.width, 1)).collect();
    (popup, rows, first)
}

pub(super) fn draw_log(f: &mut Frame, app: &App, sel: usize) {
    use crate::events::Kind;
    let th = &app.theme;
    let total = app.events.items.len();
    let (popup, rows, first) = log_layout(f.area(), total, sel);
    clear(f, popup, th);
    f.render_widget(popup_block(th, "Лента событий"), popup);
    if total == 0 {
        f.render_widget(
            Paragraph::new(Span::styled(
                "Пока пусто: события появятся, когда агенты что-то сделают",
                Style::default().fg(th.dim),
            )),
            Rect::new(popup.x + 3, popup.y + 2, popup.width.saturating_sub(4), 1),
        );
    }
    for (i, r) in rows.iter().enumerate() {
        let idx = first + i;
        let Some(e) = app.events.items.iter().rev().nth(idx) else { break };
        let active = idx == sel;
        let base = if active { Style::default().bg(th.active_row_bg) } else { Style::default() };
        let color = match e.kind {
            Kind::Done => th.green,
            Kind::Waiting => th.accent,
            Kind::Exited => th.red,
            Kind::Started => th.blue,
            Kind::Git => th.mauve,
            Kind::Approved => th.teal,
        };
        let w = r.width.saturating_sub(15) as usize;
        let mut text = e.text.replace('\n', " ");
        if !e.agent.is_empty() {
            text = format!("{} · {}", e.agent, text);
        }
        while text.width() > w {
            text.pop();
        }
        let line = Line::from(vec![
            Span::styled(if active { " ▸ " } else { "   " }, base.fg(th.accent)),
            Span::styled(format!("{}  ", e.time), base.fg(th.dim)),
            Span::styled(format!("{} ", e.kind.icon()), base.fg(color)),
            Span::styled(text, base.fg(th.text)),
        ]);
        f.render_widget(Paragraph::new(line).style(base), *r);
    }
    let hint = Rect::new(popup.x + 3, popup.bottom().saturating_sub(2), popup.width.saturating_sub(4), 1);
    f.render_widget(
        Paragraph::new(Span::styled(
            "↑/↓ — выбор · Enter или клик — к агенту · Esc — закрыть",
            Style::default().fg(th.dim),
        )),
        hint,
    );
}

pub(super) fn draw_integrations(f: &mut Frame, app: &App, sel: usize) {
    use crate::integrations::State;
    let th = &app.theme;
    let rows = app.integration_rows();
    let (popup, rects) = integrations_layout(f.area(), rows.len());
    clear(f, popup, th);
    f.render_widget(popup_block(th, "Настройки"), popup);
    draw_tabs(f, popup, 1, th);
    f.render_widget(
        Paragraph::new(Span::styled("Точные статусы агентов вместо разбора экрана", Style::default().fg(th.dim))),
        Rect::new(popup.x + 3, popup.y + 2, popup.width.saturating_sub(4), 1),
    );
    for (i, (row, r)) in rows.iter().zip(&rects).enumerate() {
        let active = i == sel;
        let base = if active { Style::default().bg(th.active_row_bg) } else { Style::default() };
        let (mark, mark_color) = match row.state {
            Some(State::Builtin) | Some(State::Installed) => ("✓", th.accent),
            _ => ("–", th.dim),
        };
        let state_txt = match row.state {
            Some(State::Builtin) => "встроено",
            Some(State::Installed) => "включено",
            Some(State::NotInstalled) => "выключено",
            None => "нет интеграции",
        };
        let found = if row.found { "установлен" } else { "не найден" };
        let w = r.width as usize;
        let right = format!("{state_txt:<16}{found:<10}");
        let name = row.name.clone();
        let gap = w.saturating_sub(5 + name.width() + right.width() + 1);
        let line = Line::from(vec![
            Span::styled(if active { " ▸ " } else { "   " }, base.fg(th.accent)),
            Span::styled(format!("{mark} "), base.fg(mark_color)),
            Span::styled(name, base.add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
            Span::styled(" ".repeat(gap), base),
            Span::styled(right, base.fg(if row.found { th.text } else { th.dim })),
            Span::styled(" ", base),
        ]);
        f.render_widget(Paragraph::new(line).style(base), *r);
    }
    let detail = rows.get(sel).map(|r| r.path.clone()).unwrap_or_default();
    let hint = Rect::new(popup.x + 3, popup.bottom().saturating_sub(4), popup.width.saturating_sub(5), 3);
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(detail, Style::default().fg(th.dim))),
            Line::from(Span::styled(
                "↑/↓ — выбор · Enter/Space/клик — включить/выключить",
                Style::default().fg(th.dim),
            )),
            Line::from(Span::styled(
                "Tab — раздел · Esc — закрыть · агента нужно перезапустить",
                Style::default().fg(th.dim),
            )),
        ]),
        hint,
    );
}
