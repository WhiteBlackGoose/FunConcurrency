use std::alloc::{alloc, dealloc, Layout};
use std::mem::forget;
use std::sync::atomic::*;

use lock::{Lock, LockSharedGuard};

mod lock;

struct AVecInner<T> {
    data: *mut T,
    cap: usize,
}

struct AVec<T> {
    lock: Lock<AVecInner<T>>,
    len: AtomicUsize,
}

impl<T: Send + Sync> AVec<T> {
    fn ensure_cap(&self, cap: usize) -> LockSharedGuard<'_, AVecInner<T>, AVecInner<T>> {
        let inner = self.lock.lock_shared();
        if inner.cap < cap {
            let inner = inner.upgrade();
            let new_inner = AVecInner {
                data: unsafe { alloc(Layout::array::<T>(inner.cap * 2).unwrap()) },
                cap: inner.cap * 2,
            };
            unsafe {
                std::ptr::copy(inner.data as *const T, new_inner.data as *mut T, inner.cap);
            }
            unsafe {
                dealloc(
                    inner.data as *mut u8,
                    Layout::array::<T>(inner.cap).unwrap(),
                );
            }
            inner.downgrade()
        } else {
            inner
        }
    }

    fn push(&self, el: T) {
        let new_len = self.len.fetch_add(1, Ordering::Relaxed);
        let inner = self.ensure_cap(new_len);
        unsafe {
            std::ptr::copy(&el as *const T, inner.data.add(new_len), 1);
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

    fn new() -> Self {
        Self {
            lock: Lock::new(AVecInner {
                data: unsafe { alloc(Layout::array::<T>(1).unwrap()) as *mut T },
                cap: 1,
            }),
            len: AtomicUsize::new(0),
        }
    }

    fn get(&self, index: usize) -> Option<LockSharedGuard<'_, AVecInner<T>, T>> {
        if index >= self.len.load(Ordering::Relaxed) {
            return None;
        }
        let inner = self.lock.lock_shared();
        Some(inner.map(|inner| unsafe { &*inner.data.add(index) }))
    }

    fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }
}

unsafe impl<T: Send + Sync> Send for AVec<T> {}
unsafe impl<T: Sync> Sync for AVec<T> {}

fn main() {
    let avec = AVec::new();
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
    let avec = AVec::new();
    const THREAD_COUNT: usize = 10;
    const ELEMENT_COUNT: usize = 2;
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
        (THREAD_COUNT * ELEMENT_COUNT) * (THREAD_COUNT * ELEMENT_COUNT + 1) / 2
    );
}
