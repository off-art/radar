//! Лента событий: что произошло с агентами, пока вы смотрели в другое место.
//! Хранится в памяти (до `CAP` записей), между запусками не сохраняется.

use std::collections::VecDeque;

const CAP: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Started,
    Done,
    Waiting,
    Exited,
    Git,
    Approved,
}

impl Kind {
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Started => "▶",
            Kind::Done => "✓",
            Kind::Waiting => "?",
            Kind::Exited => "■",
            Kind::Git => "⎇",
            Kind::Approved => "✔",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Event {
    /// Время «ЧЧ:ММ:СС» по местным часам.
    pub time: String,
    /// Идентификатор агента (`0` — событие без агента, например git-результат).
    pub session: u32,
    pub agent: String,
    pub kind: Kind,
    pub text: String,
}

#[derive(Default)]
pub struct Log {
    pub items: VecDeque<Event>,
    unseen: usize,
}

impl Log {
    pub fn push(&mut self, session: u32, agent: impl Into<String>, kind: Kind, text: impl Into<String>) {
        if self.items.len() == CAP {
            self.items.pop_front();
        }
        self.items.push_back(Event { time: now_hms(), session, agent: agent.into(), kind, text: text.into() });
        self.unseen = (self.unseen + 1).min(CAP);
    }

    /// Сколько событий добавилось после последнего открытия ленты.
    pub fn unseen(&self) -> usize {
        self.unseen
    }

    pub fn mark_seen(&mut self) {
        self.unseen = 0;
    }
}

/// «ЧЧ:ММ:СС» местного времени (без внешних зависимостей).
pub fn now_hms() -> String {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// «4:12» или «1:02:03» из длительности в секундах.
pub fn fmt_dur(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_and_unseen() {
        let mut l = Log::default();
        for i in 0..250 {
            l.push(1, "A", Kind::Done, format!("e{i}"));
        }
        assert_eq!(l.items.len(), CAP);
        assert_eq!(l.items.front().unwrap().text, "e50");
        assert_eq!(l.unseen(), CAP);
        l.mark_seen();
        assert_eq!(l.unseen(), 0);
        l.push(0, "", Kind::Git, "x");
        assert_eq!(l.unseen(), 1);
    }

    #[test]
    fn durations_and_clock() {
        assert_eq!(fmt_dur(252), "4:12");
        assert_eq!(fmt_dur(3723), "1:02:03");
        let t = now_hms();
        assert_eq!(t.len(), 8);
        assert_eq!(&t[2..3], ":");
    }
}
