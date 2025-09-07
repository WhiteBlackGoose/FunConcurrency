use std::cell::UnsafeCell;
use std::hint;
use std::mem::forget;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::*;

pub struct Lock<T> {
    val: AtomicU64,
    data: UnsafeCell<T>,
}

pub struct LockSharedGuard<'a, T, F> {
    rf: &'a F,
    inner: &'a Lock<T>,
}

impl<'a, T, F> Drop for LockSharedGuard<'a, T, F> {
    fn drop(&mut self) {
        self.inner.val.fetch_sub(1, Ordering::Relaxed);
    }
}

impl<'a, T, F> Deref for LockSharedGuard<'a, T, F> {
    type Target = F;

    fn deref(&self) -> &Self::Target {
        self.rf
    }
}

impl<'a, T> LockSharedGuard<'a, T, T> {
    fn new(inner: &'a Lock<T>) -> Self {
        Self::new_mapped(inner, |a| a)
    }
}

impl<'a, T, F> LockSharedGuard<'a, T, F> {
    pub fn upgrade(self) -> LockExclusiveGuard<'a, T> {
        let lock = self.inner;
        drop(self);
        lock.lock_exclusive()
    }

    pub fn new_mapped(inner: &'a Lock<T>, map: impl Fn(&'a T) -> &'a F) -> Self {
        Self {
            inner,
            rf: map(unsafe { &*inner.data.get() }),
        }
    }

    pub fn map<U>(self, map: impl Fn(&'a F) -> &'a U) -> LockSharedGuard<'a, T, U> {
        let inner = self.inner;
        let rf = self.rf;
        forget(self);
        LockSharedGuard { rf: map(rf), inner }
    }
}

// -------------------------------------------------

pub struct LockExclusiveGuard<'a, T> {
    inner: &'a Lock<T>,
}

impl<'a, T> Drop for LockExclusiveGuard<'a, T> {
    fn drop(&mut self) {
        self.inner.val.store(0, Ordering::Relaxed);
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
    pub fn downgrade(self) -> LockSharedGuard<'a, T, T> {
        self.inner.val.store(1, Ordering::Relaxed);
        let inner = self.inner;
        forget(self);
        LockSharedGuard::new(inner)
    }
}

impl<T> Lock<T> {
    const LOCK_FREE: u64 = 0;
    const LOCK_ALLOC: u64 = 0x1 << 63;

    pub fn lock_shared(&self) -> LockSharedGuard<'_, T, T> {
        let mut current = 0;
        let mut target = 1;
        loop {
            match self
                .val
                .compare_exchange(current, target, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(Self::LOCK_ALLOC) => {
                    current = 0;
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
                Ordering::Relaxed,
                Ordering::Relaxed,
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
