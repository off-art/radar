//! Панели агентов: экран терминала, заголовок, приветствие.

use super::{fit, icon, push_fit, status_color, status_time};
use crate::app::{App, Mode, PaneRect};
use crate::paths::short_path;
use crate::session::Session;
use crate::status::Status;
use crate::sync::MutexExt;
use crate::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub(super) fn conv(c: vt100::Color, th: &Theme, fg: bool) -> Color {
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
pub(super) fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}' | '\u{2060}' | '\u{FEFF}')
}

pub(super) fn render_screen(
    buf: &mut Buffer,
    area: Rect,
    s: &Session,
    th: &Theme,
    sel: Option<((u16, u16), (u16, u16))>,
) {
    let mut p = s.parser.lock_or_recover();
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

pub(super) fn draw_pane(f: &mut Frame, app: &App, pane: &PaneRect) {
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
            Span::styled(format!("{} ", s.name), Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(format!("· {} {} ", s.status.label(), status_time(s)), Style::default().fg(col)),
        ]);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border).add_modifier(if focused {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }))
            .title(title);
        f.render_widget(block, pane.outer);
    }

    let sel = app.sel.filter(|x| x.idx == pane.idx).map(|x| x.ordered());
    render_screen(f.buffer_mut(), pane.inner, s, th, sel);

    if s.status == Status::Exited && pane.inner.height > 1 {
        let y = pane.inner.bottom() - 1;
        let code = s.exit_code.unwrap_or(0);
        let msg = format!(
            " процесс завершён (код {code}) · ПКМ — перезапустить или закрыть · {} затем R / x ",
            app.prefix_label()
        );
        let r = Rect::new(pane.inner.x, y, pane.inner.width, 1);
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(fit(&msg, r.width as usize)).style(Style::default().fg(th.on_color).bg(th.red)),
            r,
        );
    } else if s.scroll > 0 && pane.inner.height > 1 {
        let msg = format!(" прокрутка ↑{} · колесо вниз или ввод — вернуться ", s.scroll);
        let w = msg.width() as u16;
        let r = Rect::new(pane.inner.right().saturating_sub(w), pane.inner.y, w.min(pane.inner.width), 1);
        f.render_widget(Paragraph::new(msg).style(Style::default().fg(th.on_color).bg(th.dim)), r);
    }

    // курсор — только у активной панели
    if focused && matches!(app.mode, Mode::Normal) && s.is_running() && s.scroll == 0 {
        let p = s.parser.lock_or_recover();
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
pub(super) fn draw_header(buf: &mut Buffer, area: Rect, s: &Session, ms: u128, th: &Theme) {
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

pub(super) fn draw_welcome(f: &mut Frame, app: &App) {
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
