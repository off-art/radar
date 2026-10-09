//! Общие части модальных окон: рамка, поля ввода, кнопки, области для кликов.

use super::clear;
use crate::textfield::TextField;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub(super) fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

pub(super) fn popup_block(th: &Theme, title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th.accent))
        .title(Span::styled(format!(" {title} "), Style::default().add_modifier(Modifier::BOLD)))
}

/// Поле ввода с видимым курсором.
pub(super) fn field_spans(t: &TextField, active: bool) -> Vec<Span<'static>> {
    let (before, at, after) = t.parts();
    if !active {
        return vec![Span::raw(t.text())];
    }
    let cur = Style::default().add_modifier(Modifier::REVERSED);
    vec![Span::raw(before), Span::styled(at.map_or_else(|| " ".into(), |c| c.to_string()), cur), Span::raw(after)]
}

/// Области для кликов в модальном окне.
#[derive(Default)]
pub struct Hit {
    pub popup: Rect,
    /// Кнопки по порядку (первая — «да»).
    pub buttons: Vec<Rect>,
    /// Строки списка (варианты, агенты, команды): (номер, область).
    pub rows: Vec<(usize, Rect)>,
    /// Поле ввода.
    pub field: Rect,
}

/// Ряд кнопок «клавиша название». Области — относительно начала строки (x от 0, y = 0).
pub(super) fn button_row(th: &Theme, items: &[(&str, &str)]) -> (Vec<Span<'static>>, Vec<Rect>) {
    let mut spans = vec![Span::raw(" ")];
    let mut rects = vec![];
    let mut x = 1u16;
    for (n, (key, label)) in items.iter().enumerate() {
        let ks = if n == 0 {
            Style::default().fg(th.on_color).bg(th.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.on_color).bg(th.dim)
        };
        let (k, l) = (format!(" {key} "), format!(" {label}"));
        let w = (k.width() + l.width()) as u16;
        spans.push(Span::styled(k, ks));
        spans.push(Span::styled(l, Style::default().fg(th.text)));
        rects.push(Rect::new(x, 0, w, 1));
        spans.push(Span::raw("      "));
        x += w + 6;
    }
    (spans, rects)
}

/// Переносит область из координат строки окна в экранные.
pub(super) fn at(inner: Rect, line: usize, r: Rect) -> Rect {
    Rect::new(inner.x + r.x, inner.y + line as u16, r.width, 1)
}

pub(super) fn inner_of(th: &Theme, r: Rect) -> Rect {
    popup_block(th, "").inner(r)
}

pub fn input_hit(th: &Theme, area: Rect) -> Hit {
    let r = centered(area, 50, 5);
    let inner = inner_of(th, r);
    let (_, rects) = button_row(th, &[("Enter", "Сохранить"), ("Esc", "Отмена")]);
    Hit {
        popup: r,
        buttons: rects.into_iter().map(|b| at(inner, 2, b)).collect(),
        field: Rect::new(inner.x, inner.y, inner.width, 1),
        ..Default::default()
    }
}

pub(super) fn draw_input_popup(f: &mut Frame, title: &str, value: &TextField, th: &Theme) {
    let r = centered(f.area(), 50, 5);
    clear(f, r, th);
    let block = popup_block(th, title);
    let inner = block.inner(r);
    f.render_widget(block, r);
    let (spans, _) = button_row(th, &[("Enter", "Сохранить"), ("Esc", "Отмена")]);
    f.render_widget(
        Paragraph::new(vec![Line::from(field_spans(value, true)), Line::from(""), Line::from(spans)]),
        inner,
    );
}
