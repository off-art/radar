//! Главный цикл и настройка терминала.

use super::App;
use crate::notify::{self};
use crate::session::Msg;
use crate::ui;
use anyhow::Result;
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
    EnableMouseCapture,
};
use crossterm::execute;
use ratatui::layout::Rect;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// Выключает режимы терминала, которые Radar включает сам (мышь, вставка, фокус).
fn disable_extra_modes() {
    let _ = execute!(std::io::stdout(), DisableMouseCapture, DisableBracketedPaste, DisableFocusChange);
}

/// `ratatui::init` при панике возвращает только raw-режим и основной экран. Мышь, bracketed paste и фокус
/// включены нами — без этого хука после падения терминал засыпает пользователя escape-последовательностями.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        disable_extra_modes();
        previous(info);
    }));
}

/// Состояние курсора терминала между кадрами (Windows).
#[derive(Default)]
struct CursorState {
    shown: bool,
    pos: Option<(u16, u16)>,
    last: Option<ratatui::buffer::Buffer>,
}

/// Отрисовка кадра для Windows Terminal. `Terminal::draw` на каждом кадре заново прячет, показывает и переставляет
/// курсор, а Windows Terminal при этом сбрасывает мигание — курсор «дрожит» и мелькает у спиннеров. Здесь курсор
/// трогается только при реальных изменениях: прячется перед записью изменённых ячеек и возвращается на место после.
/// Если кадр не изменился, в терминал не уходит ничего, и курсор мигает как обычно.
fn draw_windows(terminal: &mut ratatui::DefaultTerminal, app: &App, cur: &mut CursorState) -> Result<()> {
    use crossterm::cursor::{Hide, MoveTo, Show};
    use ratatui::backend::Backend;
    use std::io::Write;

    terminal.autoresize()?;
    app.cursor.set(None);
    {
        let mut frame = terminal.get_frame();
        ui::draw(&mut frame, app);
    }
    let buf = terminal.current_buffer_mut().clone();
    let changed = cur.last.as_ref() != Some(&buf);
    let want = app.cursor.get();
    let mut out = std::io::stdout();
    if changed && cur.shown {
        let _ = execute!(out, crossterm::terminal::BeginSynchronizedUpdate, Hide);
        cur.shown = false;
    } else if changed {
        let _ = execute!(out, crossterm::terminal::BeginSynchronizedUpdate);
    }
    terminal.flush()?;
    terminal.swap_buffers();
    Backend::flush(terminal.backend_mut())?;
    match want {
        Some((x, y)) => {
            if changed || cur.pos != want || !cur.shown {
                let _ = execute!(out, MoveTo(x, y));
            }
            if !cur.shown {
                let _ = execute!(out, Show);
                cur.shown = true;
            }
        }
        None => {
            if cur.shown {
                let _ = execute!(out, Hide);
                cur.shown = false;
            }
        }
    }
    if changed {
        let _ = execute!(out, crossterm::terminal::EndSynchronizedUpdate);
    }
    let _ = out.flush();
    cur.pos = want;
    cur.last = Some(buf);
    Ok(())
}

/// Главный цикл.
pub fn run(mut app: App, rx: Receiver<Msg>, sock: PathBuf) -> Result<()> {
    let mut terminal = ratatui::init();
    if app.cfg.notifications {
        notify::prepare();
    }
    let mut out = std::io::stdout();
    let _ = execute!(out, EnableBracketedPaste, EnableFocusChange);
    if app.cfg.mouse {
        let _ = execute!(out, EnableMouseCapture);
    }
    install_panic_hook();

    let result = (|| -> Result<()> {
        let mut last_tick = Instant::now();
        let mut last_input = Instant::now();
        let mut last_draw = Instant::now() - Duration::from_secs(1);
        let mut cur = CursorState::default();
        loop {
            // Занят — быстрый опрос; в простое реже: меньше пробуждений процессора.
            let busy = app.dirty || app.needs_animation() || last_input.elapsed() < Duration::from_millis(500);
            if event::poll(Duration::from_millis(if busy { 16 } else { 50 }))? {
                last_input = Instant::now();
                loop {
                    app.on_event(event::read()?);
                    if !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            while let Ok(msg) = rx.try_recv() {
                app.on_msg(msg);
            }
            if last_tick.elapsed() >= Duration::from_millis(250) {
                app.tick();
                last_tick = Instant::now();
            }
            if app.quit {
                break;
            }
            let since = last_draw.elapsed();
            let due = (app.dirty && since >= Duration::from_millis(16))
                || (app.needs_animation() && since >= Duration::from_millis(100))
                || since >= Duration::from_secs(1);
            if due {
                let size = terminal.size()?;
                app.compute_layout(Rect::new(0, 0, size.width, size.height));
                if cfg!(windows) {
                    draw_windows(&mut terminal, &app, &mut cur)?;
                } else {
                    terminal.draw(|f| ui::draw(f, &app))?;
                }
                app.dirty = false;
                last_draw = Instant::now();
            }
        }
        Ok(())
    })();

    app.shutdown();
    disable_extra_modes();
    ratatui::restore();
    let _ = std::fs::remove_file(sock);
    result
}
