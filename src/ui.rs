//! Отрисовка интерфейса.

use crate::app::{short_path, App, Confirm, Mode, NewForm, Palette, PaneRect};
use crate::menu::Menu;
use crate::textfield::TextField;
use crate::session::Session;
use crate::status::Status;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const DIM: Color = Color::Indexed(244);
const LINE: Color = Color::Indexed(238);
const SELECTED_BG: Color = Color::Indexed(237);

pub fn status_color(s: &Session) -> Color {
    match s.status {
        Status::Starting => DIM,
        Status::Working => Color::Rgb(56, 189, 248),
        Status::Waiting => Color::Rgb(251, 191, 36),
        Status::Idle => Color::Rgb(74, 222, 128),
        Status::Exited => {
            if s.exit_code.unwrap_or(0) == 0 {
                DIM
            } else {
                Color::Rgb(248, 113, 113)
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
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
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

pub fn draw(f: &mut Frame, app: &App) {
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
        Mode::Rename(t) => draw_input_popup(f, "Переименовать агента", t),
        Mode::Confirm(c) => draw_confirm(f, app, c),
        Mode::Help => draw_help(f, app),
        Mode::Menu(m) => draw_menu(f, m),
        Mode::Palette(p) => draw_palette(f, p),
        _ => {}
    }
}

// ───────────── сайдбар ─────────────

fn draw_sidebar(f: &mut Frame, app: &App) {
    let g = &app.geo;
    if g.sidebar.width == 0 {
        return;
    }
    let ms = app.anim_ms();
    let area = g.sidebar;
    let buf = f.buffer_mut();

    for y in area.y..area.bottom() {
        if let Some(c) = buf.cell_mut((area.right() - 1, y)) {
            c.set_symbol("│").set_style(Style::default().fg(LINE));
        }
    }
    let w = area.width as usize - 1;

    let working = app.sessions.iter().filter(|s| s.status == Status::Working).count();
    let waiting = app.sessions.iter().filter(|s| s.status == Status::Waiting).count();
    let idle = app.sessions.iter().filter(|s| s.status == Status::Idle).count();
    let title = Line::from(vec![
        Span::styled(" ◎ ", Style::default().fg(Color::Rgb(217, 119, 87))),
        Span::styled("Radar", Style::default().add_modifier(Modifier::BOLD)),
    ]);
    buf.set_line(area.x, area.y, &title, w as u16);
    if g.new_btn.width > 0 {
        let btn = Line::from(Span::styled(
            " + новый ",
            Style::default().fg(Color::Black).bg(Color::Indexed(250)),
        ));
        buf.set_line(g.new_btn.x, g.new_btn.y, &btn, g.new_btn.width);
    }

    let mut sum: Vec<Span> = vec![Span::styled(
        format!(" {} в списке", app.sessions.len()),
        Style::default().fg(DIM),
    )];
    if working > 0 {
        sum.push(Span::styled(
            format!("  {} {}", SPINNER[(ms / 80) as usize % 10], working),
            Style::default().fg(Color::Rgb(56, 189, 248)),
        ));
    }
    if waiting > 0 {
        sum.push(Span::styled(
            format!("  ● {waiting}"),
            Style::default().fg(Color::Rgb(251, 191, 36)).add_modifier(Modifier::BOLD),
        ));
    }
    if idle > 0 {
        sum.push(Span::styled(
            format!("  ✓ {idle}"),
            Style::default().fg(Color::Rgb(74, 222, 128)),
        ));
    }
    buf.set_line(area.x, area.y + 1, &Line::from(sum), w as u16);

    for &(idx, rect) in &g.items {
        let s = &app.sessions[idx];
        let selected = idx == app.selected;
        let col = status_color(s);
        if selected {
            buf.set_style(rect, Style::default().bg(SELECTED_BG));
        }
        let bg = if selected { Style::default().bg(SELECTED_BG) } else { Style::default() };

        // строка 1: иконка, имя, время
        let time = status_time(s);
        let mut name_style = bg.add_modifier(Modifier::BOLD);
        if !selected && s.unread {
            name_style = name_style.fg(Color::White);
        } else if !selected {
            name_style = name_style.fg(Color::Indexed(252));
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
            l1.push(Span::styled("⊘ ", bg.fg(DIM)));
        }
        l1.push(Span::styled(time.clone(), bg.fg(DIM)));
        if s.unread {
            l1.push(Span::styled(" ●", bg.fg(Color::Rgb(56, 189, 248))));
        }
        buf.set_line(rect.x, rect.y, &Line::from(l1), w as u16);

        // строка 2: агент · статус · подсказка
        let mut left = w.saturating_sub(4);
        let mut l2: Vec<Span> = vec![Span::styled("   ", bg)];
        push_fit(&mut l2, &s.agent, bg.fg(s.color), &mut left);
        push_fit(&mut l2, " · ", bg.fg(DIM), &mut left);
        push_fit(&mut l2, s.status.label(), bg.fg(col), &mut left);
        let sub = if s.status == Status::Waiting && !s.note.is_empty() {
            s.note.clone()
        } else if s.subtitle.is_empty() {
            short_path(&s.cwd)
        } else {
            s.subtitle.clone()
        };
        push_fit(&mut l2, " · ", bg.fg(DIM), &mut left);
        push_fit(&mut l2, &sub.replace('\n', " "), bg.fg(DIM), &mut left);
        buf.set_line(rect.x, rect.y + 1, &Line::from(l2), w as u16);
    }

    if app.sessions.is_empty() {
        let hint = Line::from(Span::styled("  пока пусто", Style::default().fg(DIM)));
        buf.set_line(area.x, area.y + 3, &hint, w as u16);
    }
}

// ───────────── панели ─────────────

fn conv(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn render_screen(buf: &mut Buffer, area: Rect, s: &Session) {
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
                out.set_symbol(if text.is_empty() { " " } else { text });
                let mut st = Style::default().fg(conv(cell.fgcolor())).bg(conv(cell.bgcolor()));
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
                out.set_style(st);
            }
        }
    }
    p.screen_mut().set_scrollback(0);
}

fn draw_pane(f: &mut Frame, app: &App, pane: &PaneRect) {
    let s = &app.sessions[pane.idx];
    let focused = pane.idx == app.selected;
    let ms = app.anim_ms();
    let col = status_color(s);

    if let Some(h) = pane.header {
        draw_header(f.buffer_mut(), h, s, ms);
    } else {
        let border = if focused { col } else { LINE };
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

    render_screen(f.buffer_mut(), pane.inner, s);

    if s.status == Status::Exited && pane.inner.height > 1 {
        let y = pane.inner.bottom() - 1;
        let code = s.exit_code.unwrap_or(0);
        let msg = format!(" процесс завершён (код {code}) · ПКМ — перезапустить или закрыть · {} затем R / x ", app.prefix_label());
        let r = Rect::new(pane.inner.x, y, pane.inner.width, 1);
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(fit(&msg, r.width as usize))
                .style(Style::default().fg(Color::Black).bg(Color::Rgb(248, 113, 113))),
            r,
        );
    } else if s.scroll > 0 && pane.inner.height > 1 {
        let msg = format!(" прокрутка ↑{} · колесо вниз или ввод — вернуться ", s.scroll);
        let w = msg.width() as u16;
        let r = Rect::new(pane.inner.right().saturating_sub(w), pane.inner.y, w.min(pane.inner.width), 1);
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(Color::Black).bg(DIM)),
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
fn draw_header(buf: &mut Buffer, area: Rect, s: &Session, ms: u128) {
    let col = status_color(s);
    let base = Style::default().bg(Color::Indexed(236));
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
        Style::default().fg(col).bg(Color::Indexed(236)).add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(Color::Black).bg(col).add_modifier(Modifier::BOLD)
    };

    let mut left = w.saturating_sub(chip_text.width() + 1);
    let mut spans = vec![Span::styled(chip_text, chip_style), Span::styled(" ", base)];
    push_fit(&mut spans, &s.name, base.add_modifier(Modifier::BOLD), &mut left);
    push_fit(&mut spans, "  ", base, &mut left);
    push_fit(&mut spans, &s.agent, base.fg(s.color), &mut left);
    push_fit(&mut spans, "  ", base, &mut left);
    push_fit(&mut spans, &short_path(&s.cwd), base.fg(DIM), &mut left);
    if s.worktree.is_some() {
        push_fit(&mut spans, "  ⎇ worktree", base.fg(Color::Rgb(167, 139, 250)), &mut left);
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
    let m = app.geo.main;
    if m.width < 20 || m.height < 8 {
        return;
    }
    let lines = vec![
        Line::from(Span::styled("◎ Radar", Style::default().fg(Color::Rgb(217, 119, 87)).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from("Несколько AI-агентов в одном окне терминала"),
        Line::from(""),
        Line::from(vec![
            Span::styled("n", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("  запустить первого агента"),
        ]),
        Line::from(vec![
            Span::styled("?", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("  все горячие клавиши"),
        ]),
        Line::from(vec![
            Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("  выйти"),
        ]),
    ];
    let h = lines.len() as u16;
    let r = Rect::new(m.x, m.y + m.height.saturating_sub(h) / 2, m.width, h);
    f.render_widget(Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center), r);
}

// ───────────── статус-бар ─────────────

fn key_hint(spans: &mut Vec<Span<'static>>, key: &str, text: &str) {
    spans.push(Span::styled(
        key.to_string(),
        Style::default().fg(Color::Rgb(251, 191, 36)).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(format!(" {text}  "), Style::default().fg(DIM)));
}

fn draw_statusbar(f: &mut Frame, app: &App) {
    let r = app.geo.status;
    if r.width == 0 || r.height == 0 {
        return;
    }
    let amber = Color::Rgb(251, 191, 36);
    let mut spans: Vec<Span> = vec![];
    if let Some((t, _)) = &app.toast {
        spans.push(Span::styled(
            format!(" {t} "),
            Style::default().fg(Color::Black).bg(amber).add_modifier(Modifier::BOLD),
        ));
    } else if matches!(app.mode, Mode::Nav(_)) {
        spans.push(Span::styled(
            " НАВИГАЦИЯ ",
            Style::default().fg(Color::Black).bg(amber).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        for (k, t) in [
            ("j/k", "агент"),
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
            key_hint(&mut spans, k, t);
        }
    } else if matches!(app.mode, Mode::Normal) {
        spans.push(Span::styled(
            format!(" {} ", app.prefix_label()),
            Style::default().fg(Color::Indexed(250)).bg(Color::Indexed(238)),
        ));
        spans.push(Span::styled(" навигация  ", Style::default().fg(DIM)));
        key_hint(&mut spans, "ПКМ", "меню");
        if let Some(l) = app.cfg.keys.direct_label(crate::keys::Action::Next) {
            key_hint(&mut spans, &l, "агенты");
        }
        key_hint(&mut spans, "?", "помощь");
    }
    let right = app.notif_label();
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    if app.geo.notif_btn.width > 0 && r.width as usize > used + right.width() + 1 {
        let pad = r.width as usize - used - right.width();
        spans.push(Span::raw(" ".repeat(pad)));
        let st = if app.notifications {
            Style::default().fg(Color::Indexed(250))
        } else {
            Style::default().fg(Color::Rgb(248, 113, 113))
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

fn popup_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(251, 191, 36)))
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

fn draw_form(f: &mut Frame, app: &App, form: &NewForm) {
    let r = centered(f.area(), 78, 16);
    f.render_widget(Clear, r);
    let block = popup_block("Новый агент");
    let inner = block.inner(r);
    f.render_widget(block, r);

    let label = |text: &str, active: bool| {
        Span::styled(
            format!("{text:<8}"),
            if active {
                Style::default().fg(Color::Rgb(251, 191, 36)).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            },
        )
    };

    // выбор агента
    let mut agent_spans = vec![label("Агент", form.field == 0)];
    for (i, a) in app.cfg.agents.iter().enumerate() {
        let sel = i == form.agent;
        let st = if sel {
            Style::default().fg(Color::Black).bg(a.color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(a.color)
        };
        agent_spans.push(Span::styled(format!(" {} ", a.name), st));
        agent_spans.push(Span::raw(" "));
    }

    let dir_text = form.dir.text();
    let default_name = std::path::Path::new(dir_text.trim())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut name_line = vec![label("Имя", form.field == 2)];
    if form.name.is_empty() && form.field != 2 {
        name_line.push(Span::styled(format!("(по умолчанию: {default_name})"), Style::default().fg(DIM)));
    } else {
        name_line.extend(field_spans(&form.name, form.field == 2));
    }
    let mut dir_line = vec![label("Папка", form.field == 1)];
    dir_line.extend(field_spans(&form.dir, form.field == 1));

    let mut lines = vec![
        Line::from(agent_spans),
        Line::from(""),
        Line::from(dir_line),
        Line::from(""),
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
            "Enter — запустить · Tab/↑↓ — поле · ←/→ — агент · Ctrl+u/w/a/e — правка · Esc — отмена",
            Style::default().fg(DIM),
        )),
    ];
    if let Some(e) = &form.error {
        lines.push(Line::from(Span::styled(
            fit(e, inner.width as usize),
            Style::default().fg(Color::Rgb(248, 113, 113)),
        )));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_input_popup(f: &mut Frame, title: &str, value: &TextField) {
    let r = centered(f.area(), 50, 5);
    f.render_widget(Clear, r);
    let block = popup_block(title);
    let inner = block.inner(r);
    f.render_widget(block, r);
    f.render_widget(
        Paragraph::new(vec![
            Line::from(field_spans(value, true)),
            Line::from(""),
            Line::from(Span::styled("Enter — сохранить · Esc — отмена", Style::default().fg(DIM))),
        ]),
        inner,
    );
}

fn draw_menu(f: &mut Frame, m: &Menu) {
    let r = m.rect;
    f.render_widget(Clear, r);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Indexed(245)));
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
                Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(LINE)),
                area,
            );
            continue;
        }
        let sel = i == m.sel && it.enabled;
        let base = if sel {
            Style::default().fg(Color::Black).bg(Color::Rgb(251, 191, 36))
        } else if it.enabled {
            Style::default()
        } else {
            Style::default().fg(DIM)
        };
        let hint_style = if sel { base } else { Style::default().fg(DIM) };
        let gap = w.saturating_sub(it.label.width() + it.hint.width());
        let line = Line::from(vec![
            Span::styled(format!(" {}", it.label), base),
            Span::styled(" ".repeat(gap), base),
            Span::styled(format!("{} ", it.hint), hint_style),
        ]);
        f.render_widget(Paragraph::new(line).style(base), area);
    }
}

fn draw_palette(f: &mut Frame, p: &Palette) {
    let matches = p.matches();
    let h = (matches.len().clamp(1, 12) + 4) as u16;
    let r = centered(f.area(), 64, h);
    f.render_widget(Clear, r);
    let block = popup_block("Команды");
    let inner = block.inner(r);
    f.render_widget(block, r);
    let mut lines = vec![
        Line::from(
            std::iter::once(Span::styled("› ", Style::default().fg(Color::Rgb(251, 191, 36))))
                .chain(field_spans(&p.input, true))
                .collect::<Vec<_>>(),
        ),
        Line::from(""),
    ];
    if matches.is_empty() {
        lines.push(Line::from(Span::styled("ничего не найдено", Style::default().fg(DIM))));
    }
    let rows = inner.height.saturating_sub(2) as usize;
    let first = p.sel.saturating_sub(rows.saturating_sub(1));
    for (pos, &i) in matches.iter().enumerate().skip(first).take(rows) {
        let e = &p.entries[i];
        let sel = pos == p.sel;
        let style = if sel {
            Style::default().fg(Color::Black).bg(Color::Rgb(251, 191, 36))
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

fn draw_confirm(f: &mut Frame, app: &App, c: &Confirm) {
    let text = match c {
        Confirm::Close(i) => {
            let name = app.sessions.get(*i).map(|s| s.name.clone()).unwrap_or_default();
            format!("Закрыть агента «{name}» и остановить его процесс?")
        }
        Confirm::Quit => {
            let n = app.sessions.iter().filter(|s| s.is_running()).count();
            if n > 0 {
                format!("Выйти? Работающие агенты ({n}) будут остановлены.")
            } else {
                "Выйти из Radar?".to_string()
            }
        }
    };
    let r = centered(f.area(), (text.width() as u16 + 6).max(40), 5);
    f.render_widget(Clear, r);
    let block = popup_block("Подтверждение");
    let inner = block.inner(r);
    f.render_widget(block, r);
    f.render_widget(
        Paragraph::new(vec![
            Line::from(text),
            Line::from(""),
            Line::from(Span::styled("y / Enter — да · любая другая клавиша — отмена", Style::default().fg(DIM))),
        ]),
        inner,
    );
}

fn draw_help(f: &mut Frame, app: &App) {
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
        ("  g".into(), "сетка ⇄ один агент"),
        ("  m / M".into(), "тишина для агента / все уведомления"),
        ("  S".into(), "выбрать звук уведомлений (7 тем, с прослушиванием)"),
        ("  u / d, PgUp / PgDn".into(), "прокрутка истории агента"),
        ("  p или Space".into(), "палитра команд (поиск по действиям и агентам)"),
        ("  q".into(), "выход"),
        (format!("{p} {p}"), "отправить агенту сам префикс"),
        ("Shift+↑ / Shift+↓".into(), "переключить агента без префикса (настраивается)"),
    ];
    let r = centered(f.area(), 74, rows.len() as u16 + 6);
    f.render_widget(Clear, r);
    let block = popup_block("Горячие клавиши");
    let inner = block.inner(r);
    f.render_widget(block, r);
    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(k, t)| {
            Line::from(vec![
                Span::styled(format!("{k:<24}"), Style::default().fg(Color::Rgb(251, 191, 36))),
                Span::raw(t.to_string()),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Любая клавиша — закрыть. Клавиши и звук: ~/.config/radar/config.toml",
        Style::default().fg(DIM),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}
