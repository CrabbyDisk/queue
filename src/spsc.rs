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

impl<T> Sender<T> {
    fn try_send(&mut self, el: T) -> Option<T> {
        let (buffer, meta) =
            unsafe { (&mut self.ptr.as_mut().buffer, &mut self.ptr.as_mut().meta) };
        let head = meta.head.load(Ordering::Relaxed);
        let tail = meta.tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) == buffer.len() {
            Some(el)
        } else {
            buffer[head % buffer.len()].write(el);
            meta.head.store(head.wrapping_add(1), Ordering::Release);
            None
        }
    }
}

#[derive(Debug)]
pub struct Receiver<T> {
    ptr: NonNull<Shared<T>>,
}

impl<T> Receiver<T> {
    fn try_recv(&mut self) -> Option<T> {
        let (buffer, meta) =
            unsafe { (&mut self.ptr.as_mut().buffer, &mut self.ptr.as_mut().meta) };
        let tail = meta.tail.load(Ordering::Relaxed);
        let head = meta.head.load(Ordering::Acquire);
        // If tail == head, then the queue is empty.
        if tail == head {
            None
        } else {
            let result = unsafe {
                Some(
                    std::mem::replace(&mut buffer[tail % buffer.len()], MaybeUninit::zeroed())
                        .assume_init(),
                )
            };
            meta.tail.store(tail.wrapping_add(1), Ordering::Release);
            result
        }
    }
}

fn new<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
    let layout = Layout::new::<Meta>()
        .extend(Layout::array::<T>(cap).unwrap())
        .unwrap();
    let ptr = unsafe { NonNull::new(alloc(layout.0.pad_to_align())).unwrap() };
    unsafe {
        ptr.cast::<Meta>().write(Meta {
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
        let mut thing = dbg!(new::<u32>(10));
        thing.0.try_send(10);
        thing.0.try_send(20);
        assert_eq!(thing.1.try_recv(), Some(10));
        assert_eq!(thing.1.try_recv(), Some(20));
    }
}
