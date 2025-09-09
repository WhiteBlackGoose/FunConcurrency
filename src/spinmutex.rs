use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct SpinMutex<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

impl<T: Sync + Send> SpinMutex<T> {
    pub fn lock(&self) -> SpinMutexGuard<'_, T> {
        loop {
            if !self.locked.swap(true, Ordering::Acquire) {
                break;
            } else {
                std::hint::spin_loop();
            }
        }
        SpinMutexGuard { mt: self }
    }

    pub fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }
}

unsafe impl<T: Send + Sync> Send for SpinMutex<T> {}
unsafe impl<T: Sync> Sync for SpinMutex<T> {}

pub struct SpinMutexGuard<'a, T> {
    mt: &'a SpinMutex<T>,
}

impl<'a, T> Drop for SpinMutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mt.locked.store(false, Ordering::Release);
    }
}

impl<'a, T> Deref for SpinMutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mt.data.get() }
    }
}
impl<'a, T> DerefMut for SpinMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mt.data.get() }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread, time::Duration};

    use super::SpinMutex;

    #[test]
    fn should_unlock() {
        let m = SpinMutex::new(5);
        let _ = m.lock();
    }

    #[test]
    fn should_unlock_drop_unlock() {
        let m = SpinMutex::new(5);
        let g = m.lock();
        drop(g);
        let _g2 = m.lock();
    }

    #[test]
    fn lock_lock() {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let m = SpinMutex::new(5);
            let _g = m.lock();
            let _g2 = m.lock();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(10)).is_err());
    }
}
