//! Un cerrojo que el mismo hilo puede volver a tomar: el del escaneo y el
//! de las listas lo son, porque unas operaciones se apoyan en otras
//! (`set_songs` en `add`, el escaneo en `index_file`...).
use std::sync::{Condvar, Mutex, PoisonError};
use std::thread::ThreadId;

#[derive(Debug, Default)]
pub struct RLock {
    state: Mutex<Option<(ThreadId, usize)>>,
    free: Condvar,
}

/// Mientras vive, el cerrojo es de este hilo.
pub struct RLockGuard<'a> {
    lock: &'a RLock,
}

impl RLock {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(None),
            free: Condvar::new(),
        }
    }

    /// Lo toma (esperando si es de otro hilo; al momento si ya es de este).
    pub fn lock(&self) -> RLockGuard<'_> {
        let me = std::thread::current().id();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            match state.as_mut() {
                None => {
                    *state = Some((me, 1));
                    break;
                }
                Some((owner, n)) if *owner == me => {
                    *n += 1;
                    break;
                }
                Some(_) => {
                    state = self
                        .free
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
        RLockGuard { lock: self }
    }
}

impl Drop for RLockGuard<'_> {
    fn drop(&mut self) {
        let mut state = self
            .lock
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((_, n)) = state.as_mut() {
            *n -= 1;
            if *n == 0 {
                *state = None;
                self.lock.free.notify_one();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_same_thread_takes_it_twice_and_another_waits() {
        let lock = Arc::new(RLock::new());
        let inside = Arc::new(AtomicUsize::new(0));
        let g1 = lock.lock();
        let g2 = lock.lock();
        let (l, i) = (lock.clone(), inside.clone());
        let t = std::thread::spawn(move || {
            let _g = l.lock();
            i.store(1, Ordering::SeqCst);
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(inside.load(Ordering::SeqCst), 0);
        drop(g2);
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(inside.load(Ordering::SeqCst), 0);
        drop(g1);
        t.join().expect("hilo");
        assert_eq!(inside.load(Ordering::SeqCst), 1);
    }
}
