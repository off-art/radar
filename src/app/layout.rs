//! Раскладка окна: список, панели агентов, области для мыши.

use super::{App, Geometry, PaneRect};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

impl App {
    pub fn visible_indices(&self) -> Vec<usize> {
        let n = self.sessions.len();
        if n == 0 {
            return vec![];
        }
        if !self.grid {
            return vec![self.selected];
        }
        let per_page = 9;
        let page = self.selected / per_page;
        (page * per_page..((page + 1) * per_page).min(n)).collect()
    }

    pub fn compute_layout(&mut self, area: Rect) {
        let status = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1.min(area.height));
        let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
        let sw = if area.width < 60 {
            0
        } else if self.cfg.sidebar_width > 0 {
            self.cfg.sidebar_width.min(area.width / 2)
        } else {
            (area.width / 4).clamp(26, 36)
        };
        let sidebar = Rect::new(body.x, body.y, sw, body.height);
        let main = Rect::new(body.x + sw, body.y, body.width - sw, body.height);

        // строки списка: заголовок группы — 1 строка, агент — 3 строки + пустая; 3 строки шапка
        let rows = self.sidebar_rows();
        let rh = |r: &crate::groups::Row| if matches!(r, crate::groups::Row::Item(_)) { 4u16 } else { 1 };
        let avail = body.height.saturating_sub(4);
        let sel_row = rows
            .iter()
            .position(|r| match r {
                crate::groups::Row::Item(i) => *i == self.selected,
                crate::groups::Row::Header { first, count, collapsed, .. } => {
                    *collapsed && (*first..*first + *count).contains(&self.selected)
                }
                crate::groups::Row::Text(_) => false,
            })
            .unwrap_or(0);
        if rows.is_empty() {
            self.sidebar_first = 0;
        } else {
            self.sidebar_first = self.sidebar_first.min(sel_row);
            while self.sidebar_first < sel_row && rows[self.sidebar_first..=sel_row].iter().map(rh).sum::<u16>() > avail
            {
                self.sidebar_first += 1;
            }
        }
        let mut items = vec![];
        let mut headers = vec![];
        let mut texts = vec![];
        if sw > 0 {
            let mut y = sidebar.y + 3;
            let bottom = sidebar.y + body.height - 1; // нижняя строка — кнопка «+ новая группа»
            for r in rows.iter().skip(self.sidebar_first) {
                match r {
                    crate::groups::Row::Header { key, first, count, .. } => {
                        if y + 1 > bottom {
                            break;
                        }
                        headers.push((key.clone(), *first, *count, Rect::new(sidebar.x, y, sw - 1, 1)));
                        y += 1;
                    }
                    crate::groups::Row::Text(t) => {
                        if y + 1 > bottom {
                            break;
                        }
                        texts.push((t.clone(), Rect::new(sidebar.x, y, sw - 1, 1)));
                        y += 1;
                    }
                    crate::groups::Row::Item(idx) => {
                        if y + 3 > bottom {
                            break;
                        }
                        items.push((*idx, Rect::new(sidebar.x, y, sw - 1, 3)));
                        y += 4;
                    }
                }
            }
        }
        let new_btn = if sw > 12 { Rect::new(sidebar.x + sw - 1 - 9, sidebar.y, 9, 1) } else { Rect::default() };
        let group_btn = if sw >= 14 && body.height >= 8 {
            Rect::new(sidebar.x + 1, sidebar.y + body.height - 1, 16.min(sw - 2), 1)
        } else {
            Rect::default()
        };
        let nw = self.notif_label().width() as u16;
        let notif_btn =
            if status.width > nw + 20 { Rect::new(status.right() - nw, status.y, nw, 1) } else { Rect::default() };

        // панели
        let vis = self.visible_indices();
        let mut panes = vec![];
        if !self.grid {
            if let Some(&idx) = vis.first() {
                let header = Rect::new(main.x, main.y, main.width, 1);
                let inner = Rect::new(main.x, main.y + 1, main.width, main.height.saturating_sub(1));
                panes.push(PaneRect { idx, outer: main, header: Some(header), inner });
            }
        } else if !vis.is_empty() {
            let n = vis.len();
            let (cols, rows) = match n {
                1 => (1, 1),
                2 => (2, 1),
                3 | 4 => (2, 2),
                5 | 6 => (3, 2),
                _ => (3, 3),
            };
            let ch = main.height / rows as u16;
            for (k, &idx) in vis.iter().enumerate() {
                let (c, r) = ((k % cols) as u16, (k / cols) as u16);
                // в неполном последнем ряду панели растягиваются на всю ширину
                let in_row = (n - r as usize * cols).min(cols) as u16;
                let cw = main.width / in_row;
                let w = if c == in_row - 1 { main.width - cw * c } else { cw };
                let h = if r as usize == rows - 1 { main.height - ch * r } else { ch };
                let outer = Rect::new(main.x + c * cw, main.y + r * ch, w, h);
                let inner =
                    Rect::new(outer.x + 1, outer.y + 1, outer.width.saturating_sub(2), outer.height.saturating_sub(2));
                panes.push(PaneRect { idx, outer, header: None, inner });
            }
        }
        for p in &panes {
            if let Some(s) = self.sessions.get_mut(p.idx) {
                s.resize(p.inner.height, p.inner.width);
            }
        }
        // прокрутка не может уйти дальше реальной истории
        for s in self.sessions.iter_mut().filter(|s| s.scroll > 0) {
            let max = s.max_scrollback();
            s.scroll = s.scroll.min(max);
        }
        self.geo = Geometry { sidebar, main, status, panes, items, headers, texts, group_btn, new_btn, notif_btn };
    }
}
