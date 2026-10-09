//! Боковая панель: список агентов и групп.

use super::{fit, icon, push_fit, status_color, status_time, SPINNER};
use crate::app::App;
use crate::paths::short_path;
use crate::status::Status;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub(super) fn draw_sidebar(f: &mut Frame, app: &App) {
    let th = &app.theme;
    let g = &app.geo;
    if g.sidebar.width == 0 {
        return;
    }
    let ms = app.anim_ms();
    let area = g.sidebar;
    let buf = f.buffer_mut();
    if th.paint {
        buf.set_style(area, Style::default().bg(th.sidebar_bg).fg(th.text));
    }

    for y in area.y..area.bottom() {
        if let Some(c) = buf.cell_mut((area.right() - 1, y)) {
            if app.divider_drag {
                c.set_symbol("┃").set_style(Style::default().fg(th.accent));
            } else {
                c.set_symbol("│").set_style(Style::default().fg(th.line));
            }
        }
    }
    let w = area.width as usize - 1;

    let working = app.sessions.iter().filter(|s| s.status == Status::Working).count();
    let waiting = app.sessions.iter().filter(|s| s.status == Status::Waiting).count();
    let idle = app.sessions.iter().filter(|s| s.status == Status::Idle).count();
    let mut title_spans = vec![Span::raw(" ")];
    title_spans.extend(super::wordmark(th, th.text));
    let title = Line::from(title_spans);
    buf.set_line(area.x, area.y, &title, w as u16);
    if g.new_btn.width > 0 {
        let btn = Line::from(Span::styled(" + новый ", Style::default().fg(th.on_color).bg(th.subtext)));
        buf.set_line(g.new_btn.x, g.new_btn.y, &btn, g.new_btn.width);
    }

    let mut sum: Vec<Span> =
        vec![Span::styled(format!(" {} в списке", app.sessions.len()), Style::default().fg(th.dim))];
    if working > 0 {
        sum.push(Span::styled(
            format!("  {} {}", SPINNER[(ms / 80) as usize % 10], working),
            Style::default().fg(th.blue),
        ));
    }
    if waiting > 0 {
        sum.push(Span::styled(format!("  ● {waiting}"), Style::default().fg(th.accent).add_modifier(Modifier::BOLD)));
    }
    if idle > 0 {
        sum.push(Span::styled(format!("  ✓ {idle}"), Style::default().fg(th.green)));
    }
    buf.set_line(area.x, area.y + 1, &Line::from(sum), w as u16);

    for (t, rect) in &g.texts {
        buf.set_line(rect.x, rect.y, &Line::from(Span::styled(fit(t, w), Style::default().fg(th.dim))), w as u16);
    }
    if g.group_btn.width > 0 {
        let btn = Line::from(Span::styled(" + новая группа ", Style::default().fg(th.on_color).bg(th.dim)));
        buf.set_line(g.group_btn.x, g.group_btn.y, &btn, g.group_btn.width);
    }

    for (key, first, count, rect) in &g.headers {
        let collapsed = app.collapsed.contains(key);
        let members = &app.sessions[(*first).min(app.sessions.len())..(*first + *count).min(app.sessions.len())];
        let sel = collapsed && (*first..*first + *count).contains(&app.selected);
        let bg = if sel { Style::default().bg(th.active_row_bg) } else { Style::default() };
        buf.set_style(*rect, bg);
        let (mut wk, mut wt, mut id) = (0, 0, 0);
        for m in members {
            match m.status {
                Status::Working => wk += 1,
                Status::Waiting => wt += 1,
                Status::Idle => id += 1,
                _ => {}
            }
        }
        let mut right: Vec<Span> = vec![];
        if wk > 0 {
            right.push(Span::styled(format!(" {} {}", SPINNER[(ms / 80) as usize % 10], wk), bg.fg(th.blue)));
        }
        if wt > 0 {
            right.push(Span::styled(format!(" ● {wt}"), bg.fg(th.accent).add_modifier(Modifier::BOLD)));
        }
        if id > 0 {
            right.push(Span::styled(format!(" ✓ {id}"), bg.fg(th.green)));
        }
        let rw: usize = right.iter().map(|s| s.content.width()).sum();
        let arrow = if collapsed { "▸" } else { "▾" };
        let name_w = w.saturating_sub(3 + rw + 5);
        let label = fit(key, name_w);
        let used = 3 + label.width() + format!(" {count}").width();
        let mut l = vec![
            Span::styled(format!(" {arrow} "), bg.fg(th.subtext)),
            Span::styled(label, bg.fg(th.subtext).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {count}"), bg.fg(th.dim)),
            Span::styled(" ".repeat(w.saturating_sub(used + rw + 1)), bg),
        ];
        l.extend(right);
        buf.set_line(rect.x, rect.y, &Line::from(l), w as u16);
    }

    for &(idx, rect) in &g.items {
        let s = &app.sessions[idx];
        let selected = idx == app.selected;
        let col = status_color(th, s);
        if selected {
            buf.set_style(rect, Style::default().bg(th.active_row_bg));
        }
        let bg = if selected { Style::default().bg(th.active_row_bg) } else { Style::default() };

        // строка 1: иконка, имя, время
        let time = status_time(s);
        let mut name_style = bg.add_modifier(Modifier::BOLD);
        if !selected {
            name_style = name_style.fg(th.text);
        }
        let unread = if s.unread { " ●" } else { "" };
        let mute = if s.muted { "⊘ " } else { "" };
        let right = format!("{mute}{time}{unread}");
        let name_w = w.saturating_sub(3 + right.width() + 1);
        let mut l1 = vec![
            Span::styled(format!(" {} ", icon(s, ms)), bg.fg(col).add_modifier(Modifier::BOLD)),
            Span::styled(fit(&s.name, name_w), name_style),
        ];
        let used: usize = 3 + fit(&s.name, name_w).width();
        let pad = w.saturating_sub(used + right.width() + 1);
        l1.push(Span::styled(" ".repeat(pad), bg));
        if s.muted {
            l1.push(Span::styled("⊘ ", bg.fg(th.dim)));
        }
        l1.push(Span::styled(time.clone(), bg.fg(th.dim)));
        if s.unread {
            l1.push(Span::styled(" ●", bg.fg(th.blue)));
        }
        buf.set_line(rect.x, rect.y, &Line::from(l1), w as u16);

        // строка 2: агент · статус · подсказка
        let mut left = w.saturating_sub(4);
        let mut l2: Vec<Span> = vec![Span::styled("   ", bg)];
        push_fit(&mut l2, &s.agent, bg.fg(s.color), &mut left);
        push_fit(&mut l2, " · ", bg.fg(th.dim), &mut left);
        push_fit(&mut l2, s.status.label(), bg.fg(col), &mut left);
        let sub = if s.status == Status::Waiting && !s.note.is_empty() { s.note.clone() } else { s.subtitle.clone() };
        if !sub.is_empty() {
            push_fit(&mut l2, " · ", bg.fg(th.dim), &mut left);
            push_fit(&mut l2, &sub.replace('\n', " "), bg.fg(th.dim), &mut left);
        }
        buf.set_line(rect.x, rect.y + 1, &Line::from(l2), w as u16);

        // строка 3: git (ветка, изменения) или путь к папке
        if rect.height >= 3 {
            let mut left = w.saturating_sub(4);
            let mut l3: Vec<Span> = vec![Span::styled("   ", bg)];
            if app.can_approve(idx) {
                push_fit(&mut l3, "y — разрешить", bg.fg(th.accent).add_modifier(Modifier::BOLD), &mut left);
                push_fit(&mut l3, "  ", bg, &mut left);
            }
            match &s.git {
                Some(g) => {
                    let color = if g.dirty() { th.peach } else { th.dim };
                    push_fit(&mut l3, &g.summary(), bg.fg(color), &mut left);
                }
                None => push_fit(&mut l3, &short_path(&s.cwd), bg.fg(th.dim), &mut left),
            }
            buf.set_line(rect.x, rect.y + 2, &Line::from(l3), w as u16);
        }
    }

    if app.sessions.is_empty() {
        let hint = Line::from(Span::styled("  пока пусто", Style::default().fg(th.dim)));
        buf.set_line(area.x, area.y + 3, &hint, w as u16);
    }
}
