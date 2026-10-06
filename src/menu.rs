//! Контекстное меню (правая кнопка мыши) и палитра команд.

use crate::keys::Action;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub label: String,
    /// `None` — разделитель.
    pub action: Option<Action>,
    pub hint: String,
    pub enabled: bool,
}

impl MenuItem {
    pub fn new(label: &str, action: Action, hint: &str) -> MenuItem {
        MenuItem { label: label.into(), action: Some(action), hint: hint.into(), enabled: true }
    }
    pub fn sep() -> MenuItem {
        MenuItem { label: String::new(), action: None, hint: String::new(), enabled: false }
    }
    pub fn disabled(mut self) -> MenuItem {
        self.enabled = false;
        self
    }
    fn selectable(&self) -> bool {
        self.action.is_some() && self.enabled
    }
}

#[derive(Clone, Debug)]
pub struct Menu {
    pub items: Vec<MenuItem>,
    pub sel: usize,
    pub rect: Rect,
}

impl Menu {
    /// Меню открывается у точки (x, y) и не выходит за пределы экрана.
    pub fn new(x: u16, y: u16, items: Vec<MenuItem>, screen: Rect) -> Menu {
        let label_w = items
            .iter()
            .map(|i| i.label.width() + if i.hint.is_empty() { 0 } else { i.hint.width() + 3 })
            .max()
            .unwrap_or(10);
        let w = (label_w as u16 + 4).min(screen.width);
        let h = (items.len() as u16 + 2).min(screen.height);
        let x = x.min(screen.right().saturating_sub(w));
        let y = y.min(screen.bottom().saturating_sub(h));
        let sel = items.iter().position(MenuItem::selectable).unwrap_or(0);
        Menu { items, sel, rect: Rect::new(x, y, w, h) }
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.rect.x && x < self.rect.right() && y >= self.rect.y && y < self.rect.bottom()
    }

    /// Индекс пункта под курсором (только выбираемые).
    pub fn item_at(&self, x: u16, y: u16) -> Option<usize> {
        if !self.contains(x, y) || x == self.rect.x || x == self.rect.right() - 1 {
            return None;
        }
        let i = y.checked_sub(self.rect.y + 1)? as usize;
        self.items.get(i).filter(|it| it.selectable()).map(|_| i)
    }

    pub fn step(&mut self, delta: isize) {
        let n = self.items.len() as isize;
        if n == 0 {
            return;
        }
        let mut i = self.sel as isize;
        for _ in 0..n {
            i = (i + delta).rem_euclid(n);
            if self.items[i as usize].selectable() {
                self.sel = i as usize;
                return;
            }
        }
    }

    pub fn current(&self) -> Option<Action> {
        self.items.get(self.sel).filter(|i| i.selectable()).and_then(|i| i.action)
    }
}

/// Строка палитры команд.
#[derive(Clone, Debug)]
pub struct PaletteEntry {
    pub title: String,
    pub hint: String,
    pub action: Action,
}

/// Фильтр палитры: все слова запроса должны встретиться в названии (без учёта регистра).
pub fn filter_palette(all: &[PaletteEntry], query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    all.iter()
        .enumerate()
        .filter(|(_, e)| {
            let t = e.title.to_lowercase();
            words.iter().all(|w| t.contains(w))
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<MenuItem> {
        vec![
            MenuItem::new("Открыть", Action::Next, ""),
            MenuItem::sep(),
            MenuItem::new("Перезапустить", Action::Restart, "R").disabled(),
            MenuItem::new("Закрыть", Action::Close, "x"),
        ]
    }

    #[test]
    fn menu_stays_on_screen() {
        let m = Menu::new(78, 23, items(), Rect::new(0, 0, 80, 24));
        assert!(m.rect.right() <= 80 && m.rect.bottom() <= 24);
    }

    #[test]
    fn step_skips_separators_and_disabled() {
        let mut m = Menu::new(0, 0, items(), Rect::new(0, 0, 80, 24));
        assert_eq!(m.sel, 0);
        m.step(1);
        assert_eq!(m.sel, 3);
        m.step(1);
        assert_eq!(m.sel, 0);
        m.step(-1);
        assert_eq!(m.sel, 3);
        assert_eq!(m.current(), Some(Action::Close));
    }

    #[test]
    fn hit_testing() {
        let m = Menu::new(10, 5, items(), Rect::new(0, 0, 80, 24));
        assert_eq!(m.item_at(12, 6), Some(0));
        assert_eq!(m.item_at(12, 7), None); // разделитель
        assert_eq!(m.item_at(12, 8), None); // отключён
        assert_eq!(m.item_at(12, 9), Some(3));
        assert_eq!(m.item_at(0, 0), None);
    }

    #[test]
    fn palette_filter() {
        let all = vec![
            PaletteEntry { title: "Новый агент".into(), hint: String::new(), action: Action::NewAgent },
            PaletteEntry { title: "Закрыть агента".into(), hint: String::new(), action: Action::Close },
        ];
        assert_eq!(filter_palette(&all, "").len(), 2);
        assert_eq!(filter_palette(&all, "НОВ"), vec![0]);
        assert_eq!(filter_palette(&all, "агент закр"), vec![1]);
        assert!(filter_palette(&all, "zzz").is_empty());
    }
}
