//! Меню, палитра команд, диалоги разрешения и подтверждения, справка.

use super::popup::{at, button_row, centered, field_spans, inner_of, popup_block, Hit};
use super::{clear, fit};
use crate::app::{App, Confirm, Palette};
use crate::menu::Menu;
use crate::status::Status;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub(super) fn draw_menu(f: &mut Frame, m: &Menu, th: &Theme) {
    let r = m.rect;
    clear(f, r, th);
    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(th.dim));
    f.render_widget(block, r);
    let w = r.width.saturating_sub(4) as usize;
    for (i, it) in m.items.iter().enumerate() {
        let y = r.y + 1 + i as u16;
        if y >= r.bottom() - 1 {
            break;
        }
        let area = Rect::new(r.x + 1, y, r.width - 2, 1);
        if it.action.is_none() {
            f.render_widget(Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(th.line)), area);
            continue;
        }
        let sel = i == m.sel && it.enabled;
        let base = if sel {
            Style::default().fg(th.on_color).bg(th.accent)
        } else if it.enabled {
            Style::default()
        } else {
            Style::default().fg(th.dim)
        };
        let hint_style = if sel { base } else { Style::default().fg(th.dim) };
        let gap = w.saturating_sub(it.label.width() + it.hint.width());
        let line = Line::from(vec![
            Span::styled(format!(" {}", it.label), base),
            Span::styled(" ".repeat(gap), base),
            Span::styled(format!("{} ", it.hint), hint_style),
        ]);
        f.render_widget(Paragraph::new(line).style(base), area);
    }
}

pub(super) fn palette_geom(th: &Theme, p: &Palette, area: Rect) -> (Rect, Rect, usize, usize) {
    let h = (p.matches().len().clamp(1, 12) + 4) as u16;
    let r = centered(area, 64, h);
    let inner = inner_of(th, r);
    let rows = inner.height.saturating_sub(2) as usize;
    (r, inner, p.sel.saturating_sub(rows.saturating_sub(1)), rows)
}

pub fn palette_hit(th: &Theme, p: &Palette, area: Rect) -> Hit {
    let (r, inner, first, rows) = palette_geom(th, p, area);
    let n = p.matches().len();
    Hit {
        popup: r,
        rows: (first..n.min(first + rows))
            .map(|pos| (pos, Rect::new(inner.x, inner.y + 2 + (pos - first) as u16, inner.width, 1)))
            .collect(),
        ..Default::default()
    }
}

pub(super) fn draw_palette(f: &mut Frame, p: &Palette, th: &Theme) {
    let matches = p.matches();
    let (r, inner, first, rows) = palette_geom(th, p, f.area());
    clear(f, r, th);
    let block = popup_block(th, "Команды");
    f.render_widget(block, r);
    let mut lines = vec![
        Line::from(
            std::iter::once(Span::styled("› ", Style::default().fg(th.accent)))
                .chain(field_spans(&p.input, true))
                .collect::<Vec<_>>(),
        ),
        Line::from(""),
    ];
    if matches.is_empty() {
        lines.push(Line::from(Span::styled("ничего не найдено", Style::default().fg(th.dim))));
    }
    for (pos, &i) in matches.iter().enumerate().skip(first).take(rows) {
        let e = &p.entries[i];
        let style = if pos == p.sel { Style::default().fg(th.on_color).bg(th.accent) } else { Style::default() };
        let w = inner.width as usize;
        let gap = w.saturating_sub(e.title.width() + e.hint.width() + 2);
        lines.push(Line::from(Span::styled(format!(" {}{}{} ", e.title, " ".repeat(gap), e.hint), style)));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// Диалог разрешения: что просит агент и вопрос с вариантами — как на его экране.
pub(super) struct ApproveGeom {
    lines: Vec<Line<'static>>,
    r: Rect,
    inner: Rect,
    hit: Hit,
}

pub(super) fn approve_geom(app: &App, i: usize, note: &str, sel: Option<usize>, area: Rect) -> Option<ApproveGeom> {
    let th = &app.theme;
    let s = app.sessions.get(i)?;
    let excerpt = s.prompt_excerpt();
    let w = area.width.saturating_sub(4).clamp(30, 78);
    let text_w = w.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            format!("{} · {} просит разрешение:", s.agent, s.name),
            Style::default().fg(th.subtext),
        )),
        Line::from(Span::styled(fit(note, text_w), Style::default().add_modifier(Modifier::BOLD))),
        Line::from(""),
    ];
    let max_rows = (area.height as usize).saturating_sub(9).clamp(3, 16);
    let skip = excerpt.len().saturating_sub(max_rows);
    let (opt_idx, _) = crate::session::option_lines(&excerpt);
    let mut opt_lines: Vec<(usize, usize)> = vec![];
    for (k, l) in excerpt.iter().enumerate().skip(skip) {
        let opt = opt_idx.iter().position(|&x| x == k);
        let line = match opt {
            // варианты рисуем сами: выбор в диалоге и выбор агента могут отличаться
            Some(o) => {
                opt_lines.push((o, lines.len()));
                let text = l.trim_start_matches(|c: char| c.is_whitespace() || "›❯>●○→".contains(c));
                let chosen = sel == Some(o);
                let st = if chosen {
                    Style::default().fg(th.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(th.dim)
                };
                Line::from(Span::styled(fit(&format!("{} {text}", if chosen { "›" } else { " " }), text_w), st))
            }
            None => Line::from(Span::styled(fit(l, text_w), Style::default().fg(th.dim))),
        };
        lines.push(line);
    }
    lines.push(Line::from(""));
    let yes = if opt_idx.is_empty() { ("y", "Разрешить") } else { ("Enter", "Подтвердить") };
    let (spans, rects) = button_row(th, &[yes, ("Esc", "Отмена")]);
    let btn_line = lines.len();
    lines.push(Line::from(spans));
    let r = centered(area, w, lines.len() as u16 + 2);
    let inner = inner_of(th, r);
    let hit = Hit {
        popup: r,
        buttons: rects.into_iter().map(|b| at(inner, btn_line, b)).collect(),
        rows: opt_lines.into_iter().map(|(o, l)| (o, Rect::new(inner.x, inner.y + l as u16, inner.width, 1))).collect(),
        ..Default::default()
    };
    Some(ApproveGeom { lines, r, inner, hit })
}

pub(super) fn draw_approve(f: &mut Frame, app: &App, i: usize, note: &str, sel: Option<usize>) {
    let Some(g) = approve_geom(app, i, note, sel, f.area()) else { return };
    clear(f, g.r, &app.theme);
    f.render_widget(popup_block(&app.theme, "Подтверждение"), g.r);
    f.render_widget(Paragraph::new(g.lines), g.inner);
}

/// Содержимое окна подтверждения: заголовок, что именно, пояснение, предупреждение, название кнопки «да».
pub(super) struct ConfirmView {
    title: String,
    subject: String,
    detail: Option<String>,
    warn: Option<String>,
    yes: &'static str,
}

pub(super) fn confirm_view(app: &App, c: &Confirm) -> ConfirmView {
    let v = |title: &str, subject: String, yes: &'static str| ConfirmView {
        title: title.to_string(),
        subject,
        detail: None,
        warn: None,
        yes,
    };
    let session = |i: usize| app.sessions.get(i);
    let name = |i: usize| session(i).map(|s| format!("{} · {}", s.name, s.agent)).unwrap_or_default();
    let branch = |i: usize| session(i).and_then(|s| s.git.as_ref()).map(|g| g.branch.clone()).unwrap_or_default();
    let dirty = |i: usize| session(i).and_then(|s| s.git.as_ref()).is_some_and(crate::git::Info::dirty);
    let running = app.sessions.iter().filter(|s| s.is_running()).count();
    match c {
        Confirm::Close(i) => {
            let mut x = v("Закрыть агента?", name(*i), "Закрыть");
            match session(*i).map(|s| s.status) {
                Some(Status::Working) => x.warn = Some("Агент сейчас работает — работа прервётся".into()),
                Some(Status::Waiting) => x.warn = Some("Агент ждёт ответа".into()),
                _ => {}
            }
            x
        }
        Confirm::Push(i) => {
            let mut x = v("Отправить ветку?", format!("⎇ {}", branch(*i)), "Отправить");
            x.detail = Some("git push на сервер".into());
            x
        }
        Confirm::Merge(i) => {
            let mut x = v("Влить ветку?", format!("⎇ {}  →  основной репозиторий", branch(*i)), "Влить");
            x.detail = Some("merge --no-ff; при конфликте слияние откатится".into());
            if dirty(*i) {
                x.warn = Some("Незакоммиченные изменения в слияние не войдут".into());
            }
            x
        }
        Confirm::RemoveWorktree(i) => {
            let mut x = v("Удалить worktree?", name(*i), "Удалить");
            x.detail = Some("Агент закроется; ветка удалится, если уже влита".into());
            if dirty(*i) {
                x.warn = Some("Незакоммиченные изменения пропадут".into());
            }
            x
        }
        Confirm::DeleteGroup(i) => {
            let n = app
                .groups
                .get(*i)
                .map_or(0, |g| app.sessions.iter().filter(|s| s.group.as_deref() == Some(g.as_str())).count());
            let mut x = v("Удалить группу?", app.groups.get(*i).cloned().unwrap_or_default(), "Удалить");
            x.detail = Some(if n > 0 {
                format!("Агенты ({n}) останутся в списке без группы")
            } else {
                "Группа пустая".into()
            });
            x
        }
        Confirm::Quit => {
            let mut x = v("Выйти из Radar?", "Окно закроется".into(), "Выйти");
            if running > 0 {
                x.subject = format!("Агентов в фоне: {running}");
                x.detail = Some("Продолжат работать; остановить — radar stop".into());
            }
            x
        }
        Confirm::QuitStop => {
            let mut x = v("Остановить всех агентов?", format!("Агентов: {running}"), "Остановить");
            x.detail = Some("Все процессы завершатся, затем Radar закроется".into());
            x
        }
        Confirm::Approve(..) => v("", String::new(), ""),
    }
}

pub(super) fn confirm_geom(app: &App, c: &Confirm, area: Rect) -> (ConfirmView, Rect, Rect) {
    let cv = confirm_view(app, c);
    let foot = format!(" Enter  {}      Esc  Отмена ", cv.yes);
    let mut body_w = cv.subject.width().max(foot.width());
    for t in [&cv.detail, &cv.warn].into_iter().flatten() {
        body_w = body_w.max(t.width() + 2);
    }
    let rows = 3 + cv.detail.is_some() as u16 + cv.warn.is_some() as u16 + 1;
    let r = centered(area, (body_w.max(34) as u16 + 6).min(area.width), rows + 2);
    let inner = inner_of(&app.theme, r);
    (cv, r, inner)
}

/// Области для кликов в окне подтверждения (в том числе окна разрешения запроса).
pub fn confirm_hit(app: &App, c: &Confirm, area: Rect) -> Hit {
    if let Confirm::Approve(i, note, sel) = c {
        return approve_geom(app, *i, note, *sel, area).map(|g| g.hit).unwrap_or_default();
    }
    let (cv, r, inner) = confirm_geom(app, c, area);
    let (_, rects) = button_row(&app.theme, &[("Enter", cv.yes), ("Esc", "Отмена")]);
    let line = 2 + cv.detail.is_some() as usize + cv.warn.is_some() as usize;
    Hit { popup: r, buttons: rects.into_iter().map(|b| at(inner, line, b)).collect(), ..Default::default() }
}

pub(super) fn draw_confirm(f: &mut Frame, app: &App, c: &Confirm) {
    if let Confirm::Approve(i, note, sel) = c {
        return draw_approve(f, app, *i, note, *sel);
    }
    let th = &app.theme;
    let (cv, r, inner) = confirm_geom(app, c, f.area());
    clear(f, r, th);
    f.render_widget(popup_block(th, &cv.title), r);
    let mut lines = vec![Line::from(Span::styled(
        format!(" {}", cv.subject),
        Style::default().fg(th.text).add_modifier(Modifier::BOLD),
    ))];
    if let Some(d) = &cv.detail {
        lines.push(Line::from(Span::styled(format!(" {d}"), Style::default().fg(th.dim))));
    }
    if let Some(w) = &cv.warn {
        lines.push(Line::from(Span::styled(format!(" ⚠ {w}"), Style::default().fg(th.yellow))));
    }
    lines.push(Line::from(""));
    let (spans, _) = button_row(th, &[("Enter", cv.yes), ("Esc", "Отмена")]);
    lines.push(Line::from(spans));
    f.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn draw_help(f: &mut Frame, app: &App) {
    let th = &app.theme;
    let p = app.prefix_label();
    let nav = format!("{p}, затем…");
    let rows: Vec<(String, &str)> = vec![
        ("ПКМ".into(), "контекстное меню (агент, панель, пустое место)"),
        ("клик / колесо".into(), "выбрать агента / прокрутка (в less, vim — стрелки)"),
        (nav, "режим навигации — клавиши ниже, выход Esc"),
        ("  j / k, ↓ / ↑, Tab".into(), "следующий / предыдущий агент"),
        ("  1…9".into(), "перейти к агенту по номеру"),
        ("  n / N".into(), "новый агент / того же типа в этой папке"),
        ("  x".into(), "закрыть агента"),
        ("  r / R".into(), "переименовать / перезапустить завершившегося"),
        ("  w".into(), "к агенту, который ждёт ответа"),
        ("  G / a / o".into(), "создать группу / переместить агента в группу / свернуть группу"),
        ("  K / J".into(), "переставить агента выше / ниже в списке (или перетащить мышью)"),
        ("  g".into(), "сетка ⇄ один агент"),
        ("  m / M".into(), "тишина для агента / все уведомления"),
        ("  S".into(), "выбрать звук уведомлений (7 тем, с прослушиванием)"),
        ("  v".into(), "изменения агента (git diff): файлы слева, строки справа"),
        ("  y".into(), "разрешить запрос агента, который ждёт ответа (с диалогом и текстом запроса)"),
        ("  l".into(), "лента событий: готов, ждёт ответа, завершён; Enter — к агенту"),
        ("  ,".into(), "настройки: цветовая схема, звук, ширина списка"),
        ("  u / d, PgUp / PgDn".into(), "прокрутка истории агента"),
        ("  p или Space".into(), "палитра команд (поиск по действиям и агентам)"),
        ("  q".into(), "выход (агенты продолжат работать в фоне)"),
        ("  Q".into(), "остановить всех агентов и выйти (то же — radar stop)"),
        (format!("{p} {p}"), "отправить агенту сам префикс"),
        ("Shift+↑ / Shift+↓".into(), "переключить агента без префикса (настраивается)"),
    ];
    let r = centered(f.area(), 74, rows.len() as u16 + 6);
    clear(f, r, th);
    let block = popup_block(th, "Горячие клавиши");
    let inner = block.inner(r);
    f.render_widget(block, r);
    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(k, t)| {
            Line::from(vec![Span::styled(format!("{k:<24}"), Style::default().fg(th.accent)), Span::raw(t.to_string())])
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Любая клавиша — закрыть. Клавиши и звук: ~/.config/radar/config.toml",
        Style::default().fg(th.dim),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}
