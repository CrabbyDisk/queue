use std::{
    alloc::{Layout, alloc},
    mem::MaybeUninit,
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

struct Meta {
    // Allocation info
    tx_dropped: AtomicBool,
    rx_dropped: AtomicBool,

    // Queue info
    head: AtomicUsize,
    tail: AtomicUsize,
}

#[repr(C)]
struct Shared<T> {
    meta: Meta,
    buffer: [MaybeUninit<T>],
}

#[derive(Debug)]
pub struct Sender<T> {
    ptr: NonNull<Shared<T>>,
}

unsafe impl<T> Send for Sender<T> {}

impl<T> Sender<T> {
    pub fn try_send(&mut self, el: T) -> Option<T> {
        let shared = &mut unsafe { self.ptr.as_mut() };
        let head = shared.meta.head.load(Ordering::Relaxed);
        let tail = shared.meta.tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) == shared.buffer.len() {
            Some(el)
        } else {
            shared.buffer[head % shared.buffer.len()].write(el);
            shared
                .meta
                .head
                .store(head.wrapping_add(1), Ordering::Release);
            None
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let shared = unsafe { self.ptr.as_mut() };
        if shared.meta.rx_dropped.load(Ordering::Acquire) {
            drop(unsafe { Box::from_non_null(self.ptr) });
        } else {
            shared.meta.tx_dropped.store(true, Ordering::Release);
        }
    }
}

#[derive(Debug)]
pub struct Receiver<T> {
    ptr: NonNull<Shared<T>>,
}

impl<T> Receiver<T> {
    pub fn try_recv(&mut self) -> Option<T> {
        let shared = &mut unsafe { self.ptr.as_mut() };
        let tail = shared.meta.tail.load(Ordering::Relaxed);
        let head = shared.meta.head.load(Ordering::Acquire);
        // If tail == head, then the queue is empty.
        if tail == head {
            None
        } else {
            shared
                .meta
                .tail
                .store(tail.wrapping_add(1), Ordering::Release);
            Some(unsafe {
                std::mem::replace(
                    &mut shared.buffer[tail % shared.buffer.len()],
                    MaybeUninit::uninit(),
                )
                .assume_init()
            })
        }
    }
}

unsafe impl<T> Send for Receiver<T> {}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let shared = unsafe { self.ptr.as_mut() };
        if shared.meta.tx_dropped.load(Ordering::Acquire) {
            drop(unsafe { Box::from_non_null(self.ptr) });
        } else {
            shared.meta.rx_dropped.store(true, Ordering::Release);
        }
    }
}

pub fn new<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
    let layout = Layout::new::<Meta>()
        .extend(Layout::array::<T>(cap).unwrap())
        .unwrap();
    let ptr = NonNull::new(unsafe { alloc(layout.0.pad_to_align()) })
        .unwrap()
        .cast();
    unsafe {
        ptr.write(Meta {
            tx_dropped: false.into(),
            rx_dropped: false.into(),
            head: 0.into(),
            tail: 0.into(),
        });
    };
    let thing = NonNull::from_raw_parts(ptr, cap);
    (Sender { ptr: thing }, Receiver { ptr: thing })
}

#[cfg(test)]
mod test {
    use crate::spsc::new;

    #[test]
    fn create() {
        let (mut tx, mut rx) = new::<u32>(10);
        tx.try_send(10);
        tx.try_send(20);
        assert_eq!(rx.try_recv(), Some(10));
        assert_eq!(rx.try_recv(), Some(20));
    }
}
