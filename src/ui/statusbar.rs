//! Нижняя строка: подсказки клавиш и состояние.

use crate::app::{App, Mode};
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub(super) fn key_hint(spans: &mut Vec<Span<'static>>, key: &str, text: &str, th: &Theme) {
    spans.push(Span::styled(key.to_string(), Style::default().fg(th.accent).add_modifier(Modifier::BOLD)));
    spans.push(Span::styled(format!(" {text}  "), Style::default().fg(th.dim)));
}

pub(super) fn draw_statusbar(f: &mut Frame, app: &App) {
    let th = &app.theme;
    let r = app.geo.status;
    if r.width == 0 || r.height == 0 {
        return;
    }
    let amber = th.accent;
    if th.paint {
        f.buffer_mut().set_style(r, Style::default().bg(th.sidebar_bg).fg(th.text));
    }
    let mut spans: Vec<Span> = vec![];
    if let Some((t, _)) = &app.toast {
        spans.push(Span::styled(
            format!(" {t} "),
            Style::default().fg(th.on_color).bg(amber).add_modifier(Modifier::BOLD),
        ));
    } else if matches!(app.mode, Mode::Nav(_)) {
        spans
            .push(Span::styled(" НАВИГАЦИЯ ", Style::default().fg(th.on_color).bg(amber).add_modifier(Modifier::BOLD)));
        spans.push(Span::raw(" "));
        for (k, t) in [
            ("j/k", "агент"),
            ("J/K", "переставить"),
            ("1-9", "перейти"),
            ("n", "новый"),
            ("x", "закрыть"),
            ("r", "имя"),
            ("w", "ждущий"),
            ("g", "сетка"),
            ("p", "палитра"),
            ("?", "помощь"),
            ("Esc", "к агенту"),
        ] {
            key_hint(&mut spans, k, t, th);
        }
    } else if matches!(app.mode, Mode::Normal) {
        spans.push(Span::styled(format!(" {} ", app.prefix_label()), Style::default().fg(th.subtext).bg(th.line)));
        spans.push(Span::styled(" навигация  ", Style::default().fg(th.dim)));
        key_hint(&mut spans, "ПКМ", "меню", th);
        if let Some(l) = app.cfg.keys.direct_label(crate::keys::Action::Next) {
            key_hint(&mut spans, &l, "агенты", th);
        }
        key_hint(&mut spans, "?", "помощь", th);
    }
    let right = app.notif_label();
    let badge = match app.events.unseen() {
        0 => String::new(),
        n => format!(" лента: {n} "),
    };
    let used: usize = spans.iter().map(|s| s.content.width()).sum::<usize>() + badge.width();
    if app.geo.notif_btn.width > 0 && r.width as usize > used + right.width() + 1 {
        let pad = r.width as usize - used - right.width();
        spans.push(Span::raw(" ".repeat(pad)));
        if !badge.is_empty() {
            spans.push(Span::styled(badge, Style::default().fg(th.on_color).bg(amber)));
        }
        let st = if app.notifications { Style::default().fg(th.subtext) } else { Style::default().fg(th.red) };
        spans.push(Span::styled(right, st));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), r);
}
