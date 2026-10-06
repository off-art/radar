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
        Mode::Confirm(c) => draw_confirm(f, app, c),
        Mode::Help => draw_help(f, app),
        Mode::Menu(m) => draw_menu(f, m, &app.theme),
        Mode::Palette(p) => draw_palette(f, p, &app.theme),
        Mode::Settings(sel) => draw_settings(f, app, *sel),
        Mode::Integrations(sel) => draw_integrations(f, app, *sel),
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
            c.set_symbol("│").set_style(Style::default().fg(th.line));
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
        } else if s.subtitle.is_empty() {
            short_path(&s.cwd)
        } else {
            s.subtitle.clone()
        };
        push_fit(&mut l2, " · ", bg.fg(th.dim), &mut left);
        push_fit(&mut l2, &sub.replace('\n', " "), bg.fg(th.dim), &mut left);
        buf.set_line(rect.x, rect.y + 1, &Line::from(l2), w as u16);
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

fn render_screen(buf: &mut Buffer, area: Rect, s: &Session, th: &Theme) {
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

    render_screen(f.buffer_mut(), pane.inner, s, th);

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
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    if app.geo.notif_btn.width > 0 && r.width as usize > used + right.width() + 1 {
        let pad = r.width as usize - used - right.width();
        spans.push(Span::raw(" ".repeat(pad)));
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
    dir_line.extend(field_spans(&form.dir, form.field == 1));

    lines.extend([
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
            "Enter — запустить · клик или Tab — поле · ←/→ — агент · Esc — отмена",
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

fn draw_confirm(f: &mut Frame, app: &App, c: &Confirm) {
    let th = &app.theme;
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
    clear(f, r, th);
    let block = popup_block(th, "Подтверждение");
    let inner = block.inner(r);
    f.render_widget(block, r);
    f.render_widget(
        Paragraph::new(vec![
            Line::from(text),
            Line::from(""),
            Line::from(Span::styled("y / Enter — да · любая другая клавиша — отмена", Style::default().fg(th.dim))),
        ]),
        inner,
    );
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
        ("  g".into(), "сетка ⇄ один агент"),
        ("  m / M".into(), "тишина для агента / все уведомления"),
        ("  S".into(), "выбрать звук уведомлений (7 тем, с прослушиванием)"),
        ("  ,".into(), "настройки: цветовая схема, звук, ширина списка"),
        ("  u / d, PgUp / PgDn".into(), "прокрутка истории агента"),
        ("  p или Space".into(), "палитра команд (поиск по действиям и агентам)"),
        ("  q".into(), "выход"),
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
