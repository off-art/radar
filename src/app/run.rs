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
    #[cfg(windows)]
    let _ = execute!(std::io::stdout(), crossterm::cursor::SetCursorStyle::DefaultUserShape);
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

/// Главный цикл.
pub fn run(mut app: App, rx: Receiver<Msg>, sock: PathBuf) -> Result<()> {
    let mut terminal = ratatui::init();
    if app.cfg.notifications {
        notify::prepare();
    }
    let mut out = std::io::stdout();
    let _ = execute!(out, EnableBracketedPaste, EnableFocusChange);
    // Windows Terminal сбрасывает фазу мигания при каждой перерисовке, а во время анимации она идёт десять раз
    // в секунду: курсор «дрожит». Неподвижный курсор этого лишён.
    #[cfg(windows)]
    let _ = execute!(out, crossterm::cursor::SetCursorStyle::SteadyBlock);
    if app.cfg.mouse {
        let _ = execute!(out, EnableMouseCapture);
    }
    install_panic_hook();

    let result = (|| -> Result<()> {
        let mut last_tick = Instant::now();
        let mut last_input = Instant::now();
        let mut last_draw = Instant::now() - Duration::from_secs(1);
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
                terminal.draw(|f| ui::draw(f, &app))?;
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
