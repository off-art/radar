//! Отрисовка интерфейса.

use crate::app::{short_path, App, Confirm, Mode, NewForm, Palette, PaneRect};
use crate::menu::Menu;
use crate::textfield::TextField;
use crate::session::Session;
use crate::theme::Theme;
use crate::status::Status;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
            if (ms / 500) % 2 == 0 {
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

// ───────────── сайдбар ─────────────

fn draw_sidebar(f: &mut Frame, app: &App) {
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
    let title = Line::from(vec![
        Span::styled(" ◎ ", Style::default().fg(th.peach)),
        Span::styled("Radar", Style::default().add_modifier(Modifier::BOLD)),
    ]);
    buf.set_line(area.x, area.y, &title, w as u16);
    if g.new_btn.width > 0 {
        let btn = Line::from(Span::styled(
            " + новый ",
            Style::default().fg(th.on_color).bg(th.subtext),
        ));
        buf.set_line(g.new_btn.x, g.new_btn.y, &btn, g.new_btn.width);
    }

    let mut sum: Vec<Span> = vec![Span::styled(
        format!(" {} в списке", app.sessions.len()),
        Style::default().fg(th.dim),
    )];
    if working > 0 {
        sum.push(Span::styled(
            format!("  {} {}", SPINNER[(ms / 80) as usize % 10], working),
            Style::default().fg(th.blue),
        ));
    }
    if waiting > 0 {
        sum.push(Span::styled(
            format!("  ● {waiting}"),
            Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
        ));
    }
    if idle > 0 {
        sum.push(Span::styled(
            format!("  ✓ {idle}"),
            Style::default().fg(th.green),
        ));
    }
    buf.set_line(area.x, area.y + 1, &Line::from(sum), w as u16);

    for (t, rect) in &g.texts {
        buf.set_line(rect.x, rect.y, &Line::from(Span::styled(fit(t, w), Style::default().fg(th.dim))), w as u16);
    }
    if g.group_btn.width > 0 {
        let btn = Line::from(Span::styled(" + группа ", Style::default().fg(th.on_color).bg(th.dim)));
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
        if !selected && s.unread {
            name_style = name_style.fg(th.text);
        } else if !selected {
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
        let sub = if s.status == Status::Waiting && !s.note.is_empty() {
            s.note.clone()
        } else {
            s.subtitle.clone()
        };
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

// ───────────── панели ─────────────

fn conv(c: vt100::Color, th: &Theme, fg: bool) -> Color {
    match c {
        vt100::Color::Default => {
            if th.paint {
                if fg {
                    th.text
                } else {
                    th.bg
                }
            } else {
                Color::Reset
            }
        }
        vt100::Color::Idx(i) if i < 16 && th.remap_ansi => th.ansi(i),
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Невидимые символы, которые ломают сетку ячеек в терминале (ZWSP, word joiner, BOM).
fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}' | '\u{2060}' | '\u{FEFF}')
}

fn render_screen(buf: &mut Buffer, area: Rect, s: &Session, th: &Theme, sel: Option<((u16, u16), (u16, u16))>) {
    let mut p = s.parser.lock().unwrap();
    p.screen_mut().set_scrollback(s.scroll);
    {
        let screen = p.screen();
        let (rows, cols) = screen.size();
        for r in 0..area.height.min(rows) {
            for c in 0..area.width.min(cols) {
                let Some(cell) = screen.cell(r, c) else { continue };
                if cell.is_wide_continuation() {
                    continue;
                }
                let Some(out) = buf.cell_mut((area.x + c, area.y + r)) else {
                    continue;
                };
                let text = cell.contents();
                // агенты (Ink) рисуют курсор как «пробел + U+200B»: в ряде терминалов это квадратик
                // и сдвиг ячеек, поэтому символы нулевой ширины отбрасываем
                if text.contains(is_zero_width) {
                    let clean: String = text.chars().filter(|c| !is_zero_width(*c)).collect();
                    out.set_symbol(if clean.is_empty() { " " } else { &clean });
                } else {
                    out.set_symbol(if text.is_empty() { " " } else { text });
                }
                let mut st = Style::default().fg(conv(cell.fgcolor(), th, true)).bg(conv(cell.bgcolor(), th, false));
                if cell.bold() {
                    st = st.add_modifier(Modifier::BOLD);
                }
                if cell.italic() {
                    st = st.add_modifier(Modifier::ITALIC);
                }
                if cell.underline() {
                    st = st.add_modifier(Modifier::UNDERLINED);
                }
                if cell.inverse() {
                    st = st.add_modifier(Modifier::REVERSED);
                }
                if let Some((a, b)) = sel {
                    if (r, c) >= a && (r, c) <= b {
                        st = Style::default().fg(th.on_color).bg(th.accent);
                    }
                }
                out.set_style(st);
            }
        }
    }
    p.screen_mut().set_scrollback(0);
}

fn draw_pane(f: &mut Frame, app: &App, pane: &PaneRect) {
    let th = &app.theme;
    let s = &app.sessions[pane.idx];
    let focused = pane.idx == app.selected;
    let ms = app.anim_ms();
    let col = status_color(th, s);

    if let Some(h) = pane.header {
        draw_header(f.buffer_mut(), h, s, ms, th);
    } else {
        let border = if focused { col } else { th.line };
        let title = Line::from(vec![
            Span::styled(format!(" {} ", icon(s, ms)), Style::default().fg(col).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{} ", s.name),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("· {} {} ", s.status.label(), status_time(s)), Style::default().fg(col)),
        ]);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }))
            .title(title);
        f.render_widget(block, pane.outer);
    }

    let sel = app.sel.filter(|x| x.idx == pane.idx).map(|x| x.ordered());
    render_screen(f.buffer_mut(), pane.inner, s, th, sel);

    if s.status == Status::Exited && pane.inner.height > 1 {
        let y = pane.inner.bottom() - 1;
        let code = s.exit_code.unwrap_or(0);
        let msg = format!(" процесс завершён (код {code}) · ПКМ — перезапустить или закрыть · {} затем R / x ", app.prefix_label());
        let r = Rect::new(pane.inner.x, y, pane.inner.width, 1);
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(fit(&msg, r.width as usize))
                .style(Style::default().fg(th.on_color).bg(th.red)),
            r,
        );
    } else if s.scroll > 0 && pane.inner.height > 1 {
        let msg = format!(" прокрутка ↑{} · колесо вниз или ввод — вернуться ", s.scroll);
        let w = msg.width() as u16;
        let r = Rect::new(pane.inner.right().saturating_sub(w), pane.inner.y, w.min(pane.inner.width), 1);
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(th.on_color).bg(th.dim)),
            r,
        );
    }

    // курсор — только у активной панели
    if focused && matches!(app.mode, Mode::Normal) && s.is_running() && s.scroll == 0 {
        let p = s.parser.lock().unwrap();
        let screen = p.screen();
        if !screen.hide_cursor() {
            let (r, c) = screen.cursor_position();
            if r < pane.inner.height && c < pane.inner.width {
                f.set_cursor_position(Position::new(pane.inner.x + c, pane.inner.y + r));
            }
        }
    }
}

/// Шапка панели: здесь «живёт» статус работы агента.
fn draw_header(buf: &mut Buffer, area: Rect, s: &Session, ms: u128, th: &Theme) {
    let col = status_color(th, s);
    let base = Style::default().bg(th.header_bg);
    buf.set_style(area, base);
    let w = area.width as usize;

    let chip_text = match s.status {
        Status::Waiting => format!(" {} ждёт ответа {} ", icon(s, ms), status_time(s)),
        Status::Working => format!(" {} работает {} ", icon(s, ms), status_time(s)),
        Status::Idle => format!(" {} готов · {} назад ", icon(s, ms), status_time(s)),
        Status::Starting => format!(" {} запуск ", icon(s, ms)),
        Status::Exited => format!(" {} завершён ", icon(s, ms)),
    };
    let blink = s.status == Status::Waiting && (ms / 500) % 2 == 1;
    let chip_style = if blink {
        Style::default().fg(col).bg(th.header_bg).add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(th.on_color).bg(col).add_modifier(Modifier::BOLD)
    };

    let mut left = w.saturating_sub(chip_text.width() + 1);
    let mut spans = vec![Span::styled(chip_text, chip_style), Span::styled(" ", base)];
    push_fit(&mut spans, &s.name, base.add_modifier(Modifier::BOLD), &mut left);
    push_fit(&mut spans, "  ", base, &mut left);
    push_fit(&mut spans, &s.agent, base.fg(s.color), &mut left);
    push_fit(&mut spans, "  ", base, &mut left);
    push_fit(&mut spans, &short_path(&s.cwd), base.fg(th.dim), &mut left);
    if let Some(g) = &s.git {
        push_fit(&mut spans, "  ", base, &mut left);
        push_fit(&mut spans, &g.summary(), base.fg(if g.dirty() { th.peach } else { th.dim }), &mut left);
    }
    if s.worktree.is_some() {
        push_fit(&mut spans, "  ⎇ worktree", base.fg(th.mauve), &mut left);
    }
    buf.set_line(area.x, area.y, &Line::from(spans), area.width);

    // бегущий индикатор по верхней границе, пока агент работает
    if s.status == Status::Working && area.width > 8 {
        let span = 6usize;
        let pos = (ms / 60) as usize % (w + span);
        for k in 0..span {
            let x = pos as isize - k as isize;
            if x >= 0 && (x as usize) < w {
                if let Some(c) = buf.cell_mut((area.x + x as u16, area.y)) {
                    if c.symbol() == " " {
                        c.set_style(Style::default().bg(col));
                    }
                }
            }
        }
    }
}

fn draw_welcome(f: &mut Frame, app: &App) {
    let th = &app.theme;
    let m = app.geo.main;
    if m.width < 20 || m.height < 8 {
        return;
    }
    let lines = vec![
        Line::from(Span::styled("◎ Radar", Style::default().fg(th.peach).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from("Несколько AI-агентов в одном окне терминала"),
        Line::from(""),
        Line::from(vec![
            Span::styled("n", Style::default().fg(th.accent).add_modifier(Modifier::BOLD)),
            Span::raw("  запустить первого агента"),
        ]),
        Line::from(vec![
            Span::styled("?", Style::default().fg(th.accent).add_modifier(Modifier::BOLD)),
            Span::raw("  все горячие клавиши"),
        ]),
        Line::from(vec![
            Span::styled("q", Style::default().fg(th.accent).add_modifier(Modifier::BOLD)),
            Span::raw("  выйти"),
        ]),
    ];
    let h = lines.len() as u16;
    let r = Rect::new(m.x, m.y + m.height.saturating_sub(h) / 2, m.width, h);
    f.render_widget(Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center), r);
}

// ───────────── статус-бар ─────────────

fn key_hint(spans: &mut Vec<Span<'static>>, key: &str, text: &str, th: &Theme) {
    spans.push(Span::styled(
        key.to_string(),
        Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(format!(" {text}  "), Style::default().fg(th.dim)));
}

fn draw_statusbar(f: &mut Frame, app: &App) {
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
        spans.push(Span::styled(
            " НАВИГАЦИЯ ",
            Style::default().fg(th.on_color).bg(amber).add_modifier(Modifier::BOLD),
        ));
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
        spans.push(Span::styled(
            format!(" {} ", app.prefix_label()),
            Style::default().fg(th.subtext).bg(th.line),
        ));
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
            spans.push(Span::styled(badge.clone(), Style::default().fg(th.on_color).bg(amber)));
        }
        let st = if app.notifications {
            Style::default().fg(th.subtext)
        } else {
            Style::default().fg(th.red)
        };
        spans.push(Span::styled(right, st));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), r);
}

// ───────────── окна ─────────────

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

fn popup_block(th: &Theme, title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th.accent))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().add_modifier(Modifier::BOLD),
        ))
}

/// Поле ввода с видимым курсором.
fn field_spans(t: &TextField, active: bool) -> Vec<Span<'static>> {
    let (before, at, after) = t.parts();
    if !active {
        return vec![Span::raw(t.text())];
    }
    let cur = Style::default().add_modifier(Modifier::REVERSED);
    vec![
        Span::raw(before),
        Span::styled(at.map(|c| c.to_string()).unwrap_or_else(|| " ".into()), cur),
        Span::raw(after),
    ]
}

/// Расположение элементов формы «Новый агент» — общее для отрисовки и обработки кликов мыши.
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

const FORM_LABEL: u16 = 8;

pub fn form_layout(area: Rect, app: &App, form: &NewForm) -> FormLayout {
    let w = 78u16.min(area.width);
    // сначала раскладываем чипы по строкам, чтобы знать высоту окна
    let avail = w.saturating_sub(2 + FORM_LABEL) as usize;
    let mut items: Vec<(usize, String)> = app
        .visible_agents(form.show_all)
        .into_iter()
        .map(|i| (i, format!(" {} ", app.cfg.agents[i].name)))
        .collect();
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

fn draw_form(f: &mut Frame, app: &App, form: &NewForm) {
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
        let mut spans = vec![if ri == 0 { label("Агент", form.field == 0) } else { Span::raw(" ".repeat(FORM_LABEL as usize)) }];
        for &k in row {
            let (i, rect) = lay.chips[k];
            let text = if i == usize::MAX {
                if form.show_all { " − скрыть недоступных ".to_string() } else {
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
    let default_name = std::path::Path::new(dir_text.trim())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
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
            dir_line.push(Span::raw(dir_text.clone()));
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
                format!("[{}] отдельный git worktree (изоляция от других агентов)", if form.worktree { "x" } else { " " }),
                if form.field == 3 {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Enter — запустить · ↑/↓ — поле · Tab в «Папка» — дополнить · Esc",
            Style::default().fg(th.dim),
        )),
    ]);
    if let Some(e) = &form.error {
        lines.push(Line::from(Span::styled(
            fit(e, inner.width as usize),
            Style::default().fg(th.red),
        )));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_input_popup(f: &mut Frame, title: &str, value: &TextField, th: &Theme) {
    let r = centered(f.area(), 50, 5);
    clear(f, r, th);
    let block = popup_block(th, title);
    let inner = block.inner(r);
    f.render_widget(block, r);
    f.render_widget(
        Paragraph::new(vec![
            Line::from(field_spans(value, true)),
            Line::from(""),
            Line::from(Span::styled("Enter — сохранить · Esc — отмена", Style::default().fg(th.dim))),
        ]),
        inner,
    );
}

fn draw_group_form(f: &mut Frame, app: &App, g: &crate::app::GroupForm) {
    let th = &app.theme;
    let rows = g.ids.len().min(12) as u16;
    let r = centered(f.area(), 64, 8 + rows.max(1));
    clear(f, r, th);
    let title = if g.orig.is_some() { "Группа: имя и состав" } else { "Новая группа" };
    let block = popup_block(th, title);
    let inner = block.inner(r);
    f.render_widget(block, r);
    let mut lines = vec![
        Line::from(vec![Span::styled("Имя: ", Style::default().fg(th.dim))]),
        Line::from(field_spans(&g.name, g.focus == 0)),
        Line::from(""),
        Line::from(Span::styled("Агенты в группе:", Style::default().fg(th.dim))),
    ];
    if g.ids.is_empty() {
        lines.push(Line::from(Span::styled("  агентов пока нет — группу можно создать пустой", Style::default().fg(th.dim))));
    }
    // окно прокрутки вокруг курсора
    let first = g.cursor.saturating_sub(rows.saturating_sub(1) as usize).min(g.ids.len().saturating_sub(rows as usize));
    for (n, id) in g.ids.iter().enumerate().skip(first).take(rows as usize) {
        let Some(s) = app.sessions.iter().find(|s| s.id == *id) else { continue };
        let on = g.checked.get(n).copied().unwrap_or(false);
        let here = g.focus == 1 && n == g.cursor;
        let base = if here { Style::default().fg(th.on_color).bg(th.accent) } else { Style::default() };
        let other = s.group.as_deref().filter(|x| !on && Some(*x) != g.orig.and_then(|i| app.groups.get(i)).map(String::as_str));
        let tail = other.map(|o| format!("  (сейчас в «{o}»)")).unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(format!(" [{}] ", if on { "x" } else { " " }), base),
            Span::styled(fit(&s.name, 22), base.add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}{}", s.agent, tail), if here { base } else { Style::default().fg(th.dim) }),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tab — имя/список · Пробел — отметить · Enter — готово · Esc",
        Style::default().fg(th.dim),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_menu(f: &mut Frame, m: &Menu, th: &Theme) {
    let r = m.rect;
    clear(f, r, th);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th.dim));
    f.render_widget(block, r);
    let w = r.width.saturating_sub(4) as usize;
    for (i, it) in m.items.iter().enumerate() {
        let y = r.y + 1 + i as u16;
        if y >= r.bottom() - 1 {
            break;
        }
        let area = Rect::new(r.x + 1, y, r.width - 2, 1);
        if it.action.is_none() {
            f.render_widget(
                Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(th.line)),
                area,
            );
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

fn draw_palette(f: &mut Frame, p: &Palette, th: &Theme) {
    let matches = p.matches();
    let h = (matches.len().clamp(1, 12) + 4) as u16;
    let r = centered(f.area(), 64, h);
    clear(f, r, th);
    let block = popup_block(th, "Команды");
    let inner = block.inner(r);
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
    let rows = inner.height.saturating_sub(2) as usize;
    let first = p.sel.saturating_sub(rows.saturating_sub(1));
    for (pos, &i) in matches.iter().enumerate().skip(first).take(rows) {
        let e = &p.entries[i];
        let sel = pos == p.sel;
        let style = if sel {
            Style::default().fg(th.on_color).bg(th.accent)
        } else {
            Style::default()
        };
        let w = inner.width as usize;
        let gap = w.saturating_sub(e.title.width() + e.hint.width() + 2);
        lines.push(Line::from(Span::styled(
            format!(" {}{}{} ", e.title, " ".repeat(gap), e.hint),
            style,
        )));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// Диалог разрешения: что просит агент и вопрос с вариантами — как на его экране.
fn draw_approve(f: &mut Frame, app: &App, i: usize, note: &str, sel: Option<usize>) {
    let th = &app.theme;
    let Some(s) = app.sessions.get(i) else { return };
    let excerpt = s.prompt_excerpt();
    let area = f.area();
    let w = area.width.saturating_sub(4).min(78).max(30);
    let text_w = w.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(format!("{} · {} просит разрешение:", s.agent, s.name), Style::default().fg(th.subtext))),
        Line::from(Span::styled(fit(note, text_w), Style::default().add_modifier(Modifier::BOLD))),
        Line::from(""),
    ];
    let max_rows = (area.height as usize).saturating_sub(9).clamp(3, 16);
    let skip = excerpt.len().saturating_sub(max_rows);
    let (opt_idx, _) = crate::session::option_lines(&excerpt);
    for (k, l) in excerpt.iter().enumerate().skip(skip) {
        let opt = opt_idx.iter().position(|&x| x == k);
        let line = match opt {
            // варианты рисуем сами: выбор в диалоге и выбор агента могут отличаться
            Some(o) => {
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
    let foot = if opt_idx.is_empty() {
        "y — разрешить · любая другая клавиша — отмена"
    } else {
        "↑/↓ или цифра — выбор · Enter / y — подтвердить · Esc — отмена"
    };
    lines.push(Line::from(Span::styled(foot, Style::default().fg(th.subtext))));
    let r = centered(area, w, lines.len() as u16 + 2);
    clear(f, r, th);
    let block = popup_block(th, "Подтверждение");
    let inner = block.inner(r);
    f.render_widget(block, r);
    f.render_widget(Paragraph::new(lines), inner);
}

/// Содержимое окна подтверждения: заголовок, что именно, пояснение, предупреждение, название кнопки «да».
struct ConfirmView {
    title: String,
    subject: String,
    detail: Option<String>,
    warn: Option<String>,
    yes: &'static str,
}

fn confirm_view(app: &App, c: &Confirm) -> ConfirmView {
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
    let dirty = |i: usize| session(i).and_then(|s| s.git.as_ref()).map_or(false, |g| g.dirty());
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
            let n = app.groups.get(*i).map_or(0, |g| app.sessions.iter().filter(|s| s.group.as_deref() == Some(g.as_str())).count());
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

fn draw_confirm(f: &mut Frame, app: &App, c: &Confirm) {
    if let Confirm::Approve(i, note, sel) = c {
        return draw_approve(f, app, *i, note, *sel);
    }
    let th = &app.theme;
    let cv = confirm_view(app, c);
    let foot = format!(" Enter  {}      Esc  Отмена ", cv.yes);
    let mut body_w = cv.subject.width().max(foot.width());
    for t in [&cv.detail, &cv.warn].into_iter().flatten() {
        body_w = body_w.max(t.width() + 2);
    }
    let rows = 3 + cv.detail.is_some() as u16 + cv.warn.is_some() as u16 + 1;
    let r = centered(f.area(), (body_w.max(34) as u16 + 6).min(f.area().width), rows + 2);
    clear(f, r, th);
    let block = popup_block(th, &cv.title);
    let inner = block.inner(r);
    f.render_widget(block, r);
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
    let key = Style::default().fg(th.on_color).bg(th.accent).add_modifier(Modifier::BOLD);
    let txt = Style::default().fg(th.text);
    lines.push(Line::from(vec![
        Span::raw(" "),
        Span::styled(" Enter ", key),
        Span::styled(format!(" {}", cv.yes), txt),
        Span::raw("      "),
        Span::styled(" Esc ", Style::default().fg(th.on_color).bg(th.dim)),
        Span::styled(" Отмена", txt),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_help(f: &mut Frame, app: &App) {
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
            Line::from(vec![
                Span::styled(format!("{k:<24}"), Style::default().fg(th.accent)),
                Span::raw(t.to_string()),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Любая клавиша — закрыть. Клавиши и звук: ~/.config/radar/config.toml",
        Style::default().fg(th.dim),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}

// ───────────── настройки ─────────────

/// Окно настроек и прямоугольники его строк (общее для отрисовки и мыши).
pub fn settings_layout(area: Rect, rows: usize) -> (Rect, Vec<Rect>) {
    let popup = centered(area, 64, rows as u16 + 6);
    let inner = Rect::new(popup.x + 1, popup.y + 1, popup.width.saturating_sub(2), popup.height.saturating_sub(2));
    let rects = (0..rows).map(|i| Rect::new(inner.x, inner.y + 1 + i as u16, inner.width, 1)).collect();
    (popup, rects)
}

fn draw_settings(f: &mut Frame, app: &App, sel: usize) {
    let th = &app.theme;
    let rows = app.settings_rows();
    let (popup, rects) = settings_layout(f.area(), rows.len());
    clear(f, popup, th);
    f.render_widget(popup_block(th, "Настройки"), popup);
    draw_tabs(f, popup, 0, th);
    for (i, ((label, value), r)) in rows.iter().zip(&rects).enumerate() {
        let active = i == sel;
        let base = if active {
            Style::default().bg(th.active_row_bg)
        } else {
            Style::default()
        };
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
            Line::from(Span::styled("↑/↓ — строка · ←/→ или клик — изменить · Tab — раздел · Esc — закрыть", Style::default().fg(th.dim))),
            Line::from(Span::styled("Цветовая схема применяется сразу. Свои цвета: [theme.custom] в config.toml", Style::default().fg(th.dim))),
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

fn draw_tabs(f: &mut Frame, popup: Rect, active: usize, th: &Theme) {
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

// ───────────── лента событий ─────────────

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

fn draw_log(f: &mut Frame, app: &App, sel: usize) {
    use crate::events::Kind;
    let th = &app.theme;
    let total = app.events.items.len();
    let (popup, rows, first) = log_layout(f.area(), total, sel);
    clear(f, popup, th);
    f.render_widget(popup_block(th, "Лента событий"), popup);
    if total == 0 {
        f.render_widget(
            Paragraph::new(Span::styled("Пока пусто: события появятся, когда агенты что-то сделают", Style::default().fg(th.dim))),
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
        Paragraph::new(Span::styled("↑/↓ — выбор · Enter или клик — к агенту · Esc — закрыть", Style::default().fg(th.dim))),
        hint,
    );
}

fn draw_integrations(f: &mut Frame, app: &App, sel: usize) {
    use crate::integrations::State;
    let th = &app.theme;
    let rows = app.integration_rows();
    let (popup, rects) = integrations_layout(f.area(), rows.len());
    clear(f, popup, th);
    f.render_widget(popup_block(th, "Настройки"), popup);
    draw_tabs(f, popup, 1, th);
    f.render_widget(
        Paragraph::new(Span::styled(
            "Точные статусы агентов вместо разбора экрана",
            Style::default().fg(th.dim),
        )),
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
            Line::from(Span::styled("↑/↓ — выбор · Enter/Space/клик — включить/выключить", Style::default().fg(th.dim))),
            Line::from(Span::styled("Tab — раздел · Esc — закрыть · агента нужно перезапустить", Style::default().fg(th.dim))),
        ]),
        hint,
    );
}

// ───────────── просмотр изменений ─────────────

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
fn fit_tail(s: &str, w: usize) -> String {
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

fn draw_diff(f: &mut Frame, app: &App, v: &crate::diff::View) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_width_detected() {
        assert!(is_zero_width('\u{200B}'));
        assert!(!is_zero_width(' '));
        assert!(!is_zero_width('\u{200D}')); // ZWJ нужен для составных эмодзи
    }
}
