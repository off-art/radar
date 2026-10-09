//! Формы «Новый агент» и «Группа».

use super::popup::{at, button_row, centered, field_spans, inner_of, popup_block, Hit};
use super::{clear, fit};
use crate::app::{App, NewForm};
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub struct FormLayout {
    pub popup: Rect,
    /// (индекс агента или usize::MAX для кнопки «все/скрыть», область)
    pub chips: Vec<(usize, Rect)>,
    pub dir: Rect,
    pub name: Rect,
    pub worktree: Rect,
    /// Строки с чипами: для каждой — индексы в `chips`.
    rows: Vec<Vec<usize>>,
    inner: Rect,
}

pub(super) const FORM_LABEL: u16 = 8;

pub fn form_layout(area: Rect, app: &App, form: &NewForm) -> FormLayout {
    let w = 78u16.min(area.width);
    // сначала раскладываем чипы по строкам, чтобы знать высоту окна
    let avail = w.saturating_sub(2 + FORM_LABEL) as usize;
    let mut items: Vec<(usize, String)> =
        app.visible_agents(form.show_all).into_iter().map(|i| (i, format!(" {} ", app.cfg.agents[i].name))).collect();
    let hidden = app.cfg.agents.len() - app.visible_agents(false).len();
    if form.show_all {
        if hidden > 0 {
            items.push((usize::MAX, " − скрыть недоступных ".to_string()));
        }
    } else if hidden > 0 {
        items.push((usize::MAX, format!(" + ещё {hidden} (не установлены) ")));
    }
    let mut rows: Vec<Vec<usize>> = vec![vec![]];
    let mut used = 0;
    for (k, (_, t)) in items.iter().enumerate() {
        let tw = t.width() + 1;
        if used + tw > avail && !rows.last().unwrap().is_empty() {
            rows.push(vec![]);
            used = 0;
        }
        rows.last_mut().unwrap().push(k);
        used += tw;
    }
    let extra = rows.len() as u16 - 1;
    let popup = centered(area, w, 16 + extra);
    let inner = Rect::new(popup.x + 1, popup.y + 1, popup.width.saturating_sub(2), popup.height.saturating_sub(2));
    let mut chips = vec![];
    for (r, row) in rows.iter().enumerate() {
        let mut x = inner.x + FORM_LABEL;
        for &k in row {
            let (i, t) = &items[k];
            let tw = t.width() as u16;
            chips.push((*i, Rect::new(x, inner.y + r as u16, tw, 1)));
            x += tw + 1;
        }
    }
    let base = inner.y + extra;
    let field = |dy: u16| Rect::new(inner.x, base + dy, inner.width, 1);
    FormLayout { popup, chips, dir: field(2), name: field(4), worktree: field(6), rows, inner }
}

pub(super) fn draw_form(f: &mut Frame, app: &App, form: &NewForm) {
    let th = &app.theme;
    let lay = form_layout(f.area(), app, form);
    let r = lay.popup;
    clear(f, r, th);
    let block = popup_block(th, "Новый агент");
    f.render_widget(block, r);
    let inner = lay.inner;

    let label = |text: &str, active: bool| {
        Span::styled(
            format!("{text:<8}"),
            if active {
                Style::default().fg(th.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(th.dim)
            },
        )
    };

    // выбор агента (строки чипов)
    let mut lines: Vec<Line> = vec![];
    for (ri, row) in lay.rows.iter().enumerate() {
        let mut spans = vec![if ri == 0 {
            label("Агент", form.field == 0)
        } else {
            Span::raw(" ".repeat(FORM_LABEL as usize))
        }];
        for &k in row {
            let (i, rect) = lay.chips[k];
            let text = if i == usize::MAX {
                if form.show_all {
                    " − скрыть недоступных ".to_string()
                } else {
                    let hidden = app.cfg.agents.len() - app.visible_agents(false).len();
                    format!(" + ещё {hidden} (не установлены) ")
                }
            } else {
                format!(" {} ", app.cfg.agents[i].name)
            };
            let _ = rect;
            let st = if i == usize::MAX {
                Style::default().fg(th.dim)
            } else {
                let a = &app.cfg.agents[i];
                let missing = !app.agent_available(i);
                if i == form.agent {
                    Style::default().fg(th.on_color).bg(a.color).add_modifier(Modifier::BOLD)
                } else if missing {
                    Style::default().fg(th.dim).add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default().fg(a.color)
                }
            };
            spans.push(Span::styled(text, st));
            spans.push(Span::raw(" "));
        }
        lines.push(Line::from(spans));
    }

    let dir_text = form.dir.text();
    let default_name =
        std::path::Path::new(dir_text.trim()).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut name_line = vec![label("Имя", form.field == 2)];
    if form.name.is_empty() && form.field != 2 {
        name_line.push(Span::styled(format!("(по умолчанию: {default_name})"), Style::default().fg(th.dim)));
    } else {
        name_line.extend(field_spans(&form.name, form.field == 2));
    }
    let mut dir_line = vec![label("Папка", form.field == 1)];
    let ghost = (form.field == 1 && form.dir.cursor() == dir_text.chars().count())
        .then(|| crate::complete::suggest(&dir_text))
        .flatten();
    match ghost {
        Some(s) if s.to_lowercase().starts_with(&dir_text.to_lowercase()) => {
            // серый «хвост» продолжает ввод; курсор стоит на его первом символе
            let tail: Vec<char> = s.chars().skip(dir_text.chars().count()).collect();
            let dim = Style::default().fg(th.dim);
            dir_line.push(Span::raw(dir_text));
            dir_line.push(Span::styled(tail[0].to_string(), dim.add_modifier(Modifier::REVERSED)));
            dir_line.push(Span::styled(format!("{}  (→)", tail[1..].iter().collect::<String>()), dim));
        }
        Some(s) => {
            dir_line.extend(field_spans(&form.dir, true));
            dir_line.push(Span::styled(format!("  → {s}  (→)"), Style::default().fg(th.dim)));
        }
        None => dir_line.extend(field_spans(&form.dir, form.field == 1)),
    }

    let hint_line = if form.field == 1 && !form.hints.is_empty() {
        let w = inner.width.saturating_sub(FORM_LABEL + 2) as usize;
        let mut s = String::new();
        for h in &form.hints {
            let piece = format!("{h}/  ");
            if s.width() + piece.width() > w.saturating_sub(2) {
                s.push('…');
                break;
            }
            s.push_str(&piece);
        }
        Line::from(vec![Span::raw(" ".repeat(FORM_LABEL as usize)), Span::styled(s, Style::default().fg(th.dim))])
    } else {
        Line::from("")
    };
    lines.extend([
        Line::from(""),
        Line::from(dir_line),
        hint_line,
        Line::from(name_line),
        Line::from(""),
        Line::from(vec![
            label("", form.field == 3),
            Span::styled(
                format!(
                    "[{}] отдельный git worktree (изоляция от других агентов)",
                    if form.worktree { "x" } else { " " }
                ),
                if form.field == 3 { Style::default().add_modifier(Modifier::BOLD) } else { Style::default() },
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Enter — запустить · ↑/↓ — поле · Tab в «Папка» — дополнить · Esc",
            Style::default().fg(th.dim),
        )),
    ]);
    if let Some(e) = &form.error {
        lines.push(Line::from(Span::styled(fit(e, inner.width as usize), Style::default().fg(th.red))));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
/// Размеры окна группы: (окно, внутренняя область, первый показанный агент, число строк списка).
pub(super) fn group_geom(th: &Theme, g: &crate::app::GroupForm, area: Rect) -> (Rect, Rect, usize, usize) {
    let rows = g.ids.len().min(12);
    let r = centered(area, 64, (4 + rows.max(1) + 3 + 2) as u16);
    let first = g.cursor.saturating_sub(rows.saturating_sub(1)).min(g.ids.len().saturating_sub(rows));
    (r, inner_of(th, r), first, rows)
}

pub fn group_hit(th: &Theme, g: &crate::app::GroupForm, area: Rect) -> Hit {
    let (r, inner, first, rows) = group_geom(th, g, area);
    let (_, rects) = button_row(th, &[("Enter", "Сохранить"), ("Esc", "Отмена")]);
    let last = 4 + rows.max(1) + 2;
    Hit {
        popup: r,
        buttons: rects.into_iter().map(|b| at(inner, last, b)).collect(),
        rows: (first..first + rows)
            .map(|n| (n, Rect::new(inner.x, inner.y + (4 + n - first) as u16, inner.width, 1)))
            .collect(),
        field: Rect::new(inner.x, inner.y, inner.width, 2),
    }
}

pub(super) fn draw_group_form(f: &mut Frame, app: &App, g: &crate::app::GroupForm) {
    let th = &app.theme;
    let (r, inner, first, rows) = group_geom(th, g, f.area());
    clear(f, r, th);
    let title = if g.orig.is_some() { "Группа: имя и состав" } else { "Новая группа" };
    let block = popup_block(th, title);
    f.render_widget(block, r);
    let mut lines = vec![
        Line::from(vec![Span::styled("Имя: ", Style::default().fg(th.dim))]),
        Line::from(field_spans(&g.name, g.focus == 0)),
        Line::from(""),
        Line::from(Span::styled("Агенты в группе (клик или Пробел — отметить):", Style::default().fg(th.dim))),
    ];
    if g.ids.is_empty() {
        lines.push(Line::from(Span::styled(
            "  агентов пока нет — группу можно создать пустой",
            Style::default().fg(th.dim),
        )));
    }
    for (n, id) in g.ids.iter().enumerate().skip(first).take(rows) {
        let Some(s) = app.sessions.iter().find(|s| s.id == *id) else { continue };
        let on = g.checked.get(n).copied().unwrap_or(false);
        let here = g.focus == 1 && n == g.cursor;
        let base = if here { Style::default().fg(th.on_color).bg(th.accent) } else { Style::default() };
        let other = s
            .group
            .as_deref()
            .filter(|x| !on && Some(*x) != g.orig.and_then(|i| app.groups.get(i)).map(String::as_str));
        let tail = other.map(|o| format!("  (сейчас в «{o}»)")).unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(format!(" [{}] ", if on { "x" } else { " " }), base),
            Span::styled(fit(&s.name, 22), base.add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}{}", s.agent, tail), if here { base } else { Style::default().fg(th.dim) }),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Tab — имя/список · ↑/↓ — выбор", Style::default().fg(th.dim))));
    let (spans, _) = button_row(th, &[("Enter", "Сохранить"), ("Esc", "Отмена")]);
    lines.push(Line::from(spans));
    f.render_widget(Paragraph::new(lines), inner);
}
