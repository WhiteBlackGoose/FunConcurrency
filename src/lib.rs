use std::alloc::{alloc, dealloc, Layout};
use std::mem::{forget, MaybeUninit};
use std::ops::Deref;
use std::sync::atomic::*;

use lock::{Lock, LockSharedGuard};

pub mod lock;
pub mod spinmutex;

struct AVecInner<T> {
    data: *mut T,
    cap: usize,
    len: AtomicUsize,
}

pub struct AVec<T> {
    lock: Lock<AVecInner<T>>,
}

impl<T: Send + Sync> AVec<T> {
    fn ensure_cap<'a>(
        &'a self,
        cap: usize,
        inner: LockSharedGuard<'a, AVecInner<T>>,
    ) -> LockSharedGuard<'a, AVecInner<T>> {
        if inner.cap < cap {
            let mut inner = inner.upgrade();
            // upgrade loses the lock => we need to double check
            if inner.cap >= cap {
                return inner.downgrade();
            }
            let new_inner = AVecInner {
                data: unsafe { alloc(Layout::array::<T>(inner.cap * 2).unwrap()) as *mut T },
                cap: inner.cap * 2,
                len: AtomicUsize::new(inner.len.load(Ordering::Relaxed)),
            };
            unsafe {
                std::ptr::copy(inner.data as *const T, new_inner.data, inner.cap);
            }
            unsafe {
                dealloc(
                    inner.data as *mut u8,
                    Layout::array::<T>(inner.cap).unwrap(),
                );
            }
            *inner = new_inner;
            inner.downgrade()
        } else {
            inner
        }
    }

    pub fn push(&self, el: T) {
        let inner = self.lock.lock_shared();
        let top_element = inner.len.fetch_add(1, Ordering::Relaxed);
        let inner = self.ensure_cap(top_element + 1, inner);
        unsafe {
            std::ptr::copy(&el as *const T, inner.data.add(top_element), 1);
        }
        forget(el);
    }

    pub fn new(cap: usize) -> Self {
        Self {
            lock: Lock::new(AVecInner {
                data: unsafe { alloc(Layout::array::<T>(cap).unwrap()) as *mut T },
                cap,
                len: AtomicUsize::new(0),
            }),
        }
    }

    pub fn get(&self, index: usize) -> Option<AVecRefElement<'_, T>> {
        let inner = self.lock.lock_shared();
        if index >= inner.len.load(Ordering::Relaxed) {
            return None;
        }
        Some(AVecRefElement { inner, index })
    }

    pub fn len(&self) -> usize {
        self.lock.lock_shared().len.load(Ordering::Relaxed)
    }
}

pub struct AVecRefElement<'a, T> {
    inner: LockSharedGuard<'a, AVecInner<T>>,
    index: usize,
}

impl<'a, T> Deref for AVecRefElement<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.data.add(self.index) }
    }
}

impl<T> Drop for AVec<T> {
    fn drop(&mut self) {
        let inner = self.lock.lock_exclusive();
        let len = inner.len.load(Ordering::Relaxed);
        for i in 0..len {
            let mut el = MaybeUninit::uninit();
            unsafe {
                std::ptr::copy(
                    inner.data.add(i),
                    &mut el as *mut MaybeUninit<T> as *mut T,
                    1,
                );
                let _ = el.assume_init();
            }
        }
        unsafe {
            dealloc(
                inner.data as *mut u8,
                Layout::array::<T>(inner.cap).unwrap(),
            );
        }
    }
}

unsafe impl<T: Send + Sync> Send for AVec<T> {}
unsafe impl<T: Sync> Sync for AVec<T> {}

#[test]
fn many_threads() {
    let avec = AVec::new(1);
    const THREAD_COUNT: usize = 12;
    const ELEMENT_COUNT: usize = 20000;
    std::thread::scope(|s| {
        for _ in 0..THREAD_COUNT {
            s.spawn(|| {
                for i in 1..ELEMENT_COUNT + 1 {
                    avec.push(i);
                }
            });
        }
    });
    let mut sum = 0;
    assert_eq!(avec.len(), THREAD_COUNT * ELEMENT_COUNT);
    for i in 0..avec.len() {
        sum += *avec.get(i).unwrap();
    }
    assert_eq!(
        sum,
        THREAD_COUNT * (ELEMENT_COUNT * (ELEMENT_COUNT + 1)) / 2
    );
}
