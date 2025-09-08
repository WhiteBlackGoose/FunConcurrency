use std::alloc::{alloc, dealloc, Layout};
use std::mem::{forget, MaybeUninit};
use std::ops::Deref;
use std::sync::atomic::*;
use std::thread;

use lock::{Lock, LockSharedGuard};

mod lock;

struct AVecInner<T> {
    data: *mut T,
    cap: usize,
    len: AtomicUsize,
}

struct AVec<T> {
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

    fn push(&self, el: T) {
        let inner = self.lock.lock_shared();
        let top_element = inner.len.fetch_add(1, Ordering::Relaxed);
        let inner = self.ensure_cap(top_element + 1, inner);
        unsafe {
            std::ptr::copy(&el as *const T, inner.data.add(top_element), 1);
        }
        forget(el);
    }

    // if I want to pop, I need to have one more lock mode
    // instead of shared: shared_add and shared_remove
    /*
    fn pop(&self) -> T {
        let inner = self.lock.lock_exclusive();
        let mut el: MaybeUninit<T> = MaybeUninit::uninit();
        let top_element = self.len.fetch_sub(1, Ordering::Relaxed);
        unsafe {
            std::ptr::copy(
                inner.data.add(top_element),
                (&mut el) as *mut std::mem::MaybeUninit<T> as *mut T,
                1,
            );
            el.assume_init()
        }
    }
    */

    fn new(cap: usize) -> Self {
        Self {
            lock: Lock::new(AVecInner {
                data: unsafe { alloc(Layout::array::<T>(cap).unwrap()) as *mut T },
                cap,
                len: AtomicUsize::new(0),
            }),
        }
    }

    fn get(&self, index: usize) -> Option<AVecRefElement<'_, T>> {
        let inner = self.lock.lock_shared();
        if index >= inner.len.load(Ordering::Relaxed) {
            return None;
        }
        Some(AVecRefElement { inner, index })
    }

    fn len(&self) -> usize {
        self.lock.lock_shared().len.load(Ordering::Relaxed)
    }
}

struct AVecRefElement<'a, T> {
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

fn main() {
    let avec = AVec::new(10);
    thread::scope(|s| {
        s.spawn(|| {
            avec.push(2);
            avec.push(3);
            avec.push(4);
            let el = avec.get(2).unwrap();
            avec.push(5);
            println!("Aaa {}", *el);
            avec.push(1);
        });
    });
    avec.push(1);
    avec.push(2);
    avec.push(3);
    avec.push(4);
    for i in 0..avec.len() {
        println!("#{} element: {}", i, *avec.get(i).unwrap());
    }
}

#[test]
fn many_threads() {
    let avec = AVec::new(1);
    const THREAD_COUNT: usize = 12;
    const ELEMENT_COUNT: usize = 200;
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
