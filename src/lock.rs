use std::cell::UnsafeCell;
use std::hint;
use std::mem::forget;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::*;

pub struct Lock<T> {
    val: AtomicU64,
    data: UnsafeCell<T>,
}

pub struct LockSharedGuard<'a, T> {
    inner: &'a Lock<T>,
}

impl<'a, T> Drop for LockSharedGuard<'a, T> {
    fn drop(&mut self) {
        self.inner.val.fetch_sub(1, Ordering::AcqRel);
    }
}

impl<'a, T> Deref for LockSharedGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.data.get() }
    }
}

impl<'a, T> LockSharedGuard<'a, T> {
    fn new(inner: &'a Lock<T>) -> Self {
        Self { inner }
    }

    /// there's a gap here, make sure to double check
    /// the condition you entered it with in the first place
    pub fn upgrade(self) -> LockExclusiveGuard<'a, T> {
        let lock = self.inner;
        drop(self);
        lock.lock_exclusive()
    }
}

// -------------------------------------------------

pub struct LockExclusiveGuard<'a, T> {
    inner: &'a Lock<T>,
}

impl<'a, T> Drop for LockExclusiveGuard<'a, T> {
    fn drop(&mut self) {
        self.inner.val.store(0, Ordering::Release);
    }
}

impl<'a, T> Deref for LockExclusiveGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.data.get() }
    }
}

impl<'a, T> DerefMut for LockExclusiveGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.inner.data.get() }
    }
}

impl<'a, T> LockExclusiveGuard<'a, T> {
    /// the lock stays locked without gaps
    pub fn downgrade(self) -> LockSharedGuard<'a, T> {
        self.inner.val.store(1, Ordering::Release);
        let inner = self.inner;
        forget(self);
        LockSharedGuard::new(inner)
    }
}

// -------------------------------------------------

impl<T> Lock<T> {
    const LOCK_FREE: u64 = 0;
    const LOCK_ALLOC: u64 = 0x1 << 63;

    pub fn lock_shared(&self) -> LockSharedGuard<'_, T> {
        let mut current = Self::LOCK_FREE;
        let mut target = Self::LOCK_FREE + 1;
        loop {
            match self
                .val
                .compare_exchange(current, target, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(Self::LOCK_ALLOC) => {
                    current = 0;
                    target = 1;
                    hint::spin_loop();
                }
                Err(actual) => {
                    current = actual;
                    target = actual + 1;
                }
            }
        }
        LockSharedGuard::new(self)
    }

    pub fn lock_exclusive(&self) -> LockExclusiveGuard<'_, T> {
        loop {
            match self.val.compare_exchange(
                Self::LOCK_FREE,
                Self::LOCK_ALLOC,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(_) => {
                    hint::spin_loop();
                }
            }
        }
        LockExclusiveGuard { inner: self }
    }

    pub fn new(data: T) -> Self {
        Self {
            val: AtomicU64::new(Self::LOCK_FREE),
            data: UnsafeCell::new(data),
        }
    }
}

unsafe impl<T: Send + Sync> Send for Lock<T> {}
unsafe impl<T: Sync> Sync for Lock<T> {}

#[cfg(test)]
mod tests {
    use std::{
        ops::{Deref, DerefMut},
        sync::mpsc,
        thread,
        time::Duration,
    };

    use super::Lock;

    #[test]
    fn shared_exclusive() {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let lock = Lock::new(5);
            let _g1 = lock.lock_shared();
            let _g2 = lock.lock_exclusive();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(10)).is_err());
    }

    #[test]
    fn exclusive_shared() {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let lock = Lock::new(5);
            let _g1 = lock.lock_exclusive();
            let _g2 = lock.lock_shared();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(10)).is_err());
    }

    #[test]
    fn shared_shared() {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let lock = Lock::new(5);
            let _g1 = lock.lock_shared();
            let _g2 = lock.lock_shared();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(10)).is_ok());
    }

    #[test]
    fn shared_drop_exclusive() {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let lock = Lock::new(5);
            let g1 = lock.lock_shared();
            drop(g1);
            let _g2 = lock.lock_exclusive();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(10)).is_ok());
    }

    #[test]
    fn shared_shared_drop_exclusive() {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();
        thread::spawn(move || {
            let lock = Lock::new(5);
            let g1 = lock.lock_shared();
            let g2 = lock.lock_shared();
            let r = g1.deref();
            tx1.send(()).unwrap();
            drop(g2);
            let mut g3 = lock.lock_exclusive();
            let r2 = g3.deref_mut();
            *r2 = *r;
            tx2.send(()).unwrap();
        });
        assert!(rx1.recv_timeout(Duration::from_millis(10)).is_ok());
        assert!(rx2.recv_timeout(Duration::from_millis(10)).is_err());
    }

    #[test]
    fn shared_shared_drop_drop_exclusive() {
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();
        thread::spawn(move || {
            let lock = Lock::new(5);
            let g1 = lock.lock_shared();
            let g2 = lock.lock_shared();
            tx1.send(()).unwrap();
            drop(g1);
            drop(g2);
            let _g3 = lock.lock_exclusive();
            tx2.send(()).unwrap();
        });
        assert!(rx1.recv_timeout(Duration::from_millis(10)).is_ok());
        assert!(rx2.recv_timeout(Duration::from_millis(10)).is_ok());
    }

    #[test]
    fn exclusive_drop_shared_shared() {
        let (tx1, rx1) = mpsc::channel();
        thread::spawn(move || {
            let lock = Lock::new(5);
            let g2 = lock.lock_exclusive();
            drop(g2);
            let _g1 = lock.lock_shared();
            let _g2 = lock.lock_shared();
            tx1.send(()).unwrap();
        });
        assert!(rx1.recv_timeout(Duration::from_millis(10)).is_ok());
    }

    #[test]
    fn ub_mixed_access() {
        let v = Lock::new(5);
        thread::scope(|s| {
            for _ in 0..2 {
                s.spawn(|| {
                    for i in 0..100 {
                        *v.lock_exclusive() = i;
                        drop(v.lock_shared());
                    }
                });
            }
        });
    }
}
