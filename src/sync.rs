//! Мьютексы без паники при отравлении.
//!
//! Мьютекс «отравляется», если поток запаниковал, держа блокировку. Для Radar это не повод падать
//! всему процессу (особенно фоновому хозяину агента): данные под замками — буферы и состояние экрана,
//! которые остаются пригодными, поэтому продолжаем работать с тем, что есть.

use std::sync::{Mutex, MutexGuard};

pub trait MutexExt<T> {
    /// Блокирует мьютекс; отравление игнорируется.
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn poisoned_mutex_is_still_usable() {
        let m = Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("отравляем мьютекс");
        })
        .join();
        assert!(m.lock().is_err(), "мьютекс должен быть отравлен");
        *m.lock_or_recover() += 1;
        assert_eq!(*m.lock_or_recover(), 2);
    }
}
