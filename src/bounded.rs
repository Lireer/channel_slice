use std::{
    alloc::Layout,
    sync::{
        atomic::{AtomicPtr, AtomicUsize, Ordering},
        Arc,
    },
};

pub fn create_bounded<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let size_of_t = std::mem::size_of::<T>();
    assert_ne!(capacity, 0, "capacity is 0 but must be at least 1");
    assert_ne!(size_of_t, 0, "zero sized types are currenty not supported");
    assert!(
        !std::mem::needs_drop::<T>(),
        "types that need to be dropped are currently not supported"
    );

    let data_layout =
        Layout::from_size_align(2 * capacity * size_of_t, std::mem::align_of::<T>()).unwrap();
    let data_start: *mut T = unsafe { std::alloc::alloc(data_layout) }.cast();
    if data_start.is_null() {
        // Abort if allocation failed, see `alloc` function for more information.
        std::alloc::handle_alloc_error(data_layout);
    }

    let channel = Arc::new(ChannelData {
        data_ptr: data_start,
        len: AtomicUsize::new(0),
        capacity,
        head: AtomicPtr::new(data_start),
        tail: AtomicPtr::new(data_start),
    });

    (
        Sender {
            buf: channel.clone(),
            current_half_end: unsafe { data_start.add(capacity) },
        },
        Receiver { buf: channel },
    )
}

/// A bounded peekable channel.
///
/// # Capacity and Allocation
///
/// The capacity describes the maximum number of elements the channel can hold at a time. The amount
/// of allocated memory will almost be twice the amount required by `capacity` elements.
///
/// # How it works
///
/// `Sender` and `Writer` have a shared buffer which is logically split in two. Each half is big
/// enough to contain `capacity` elements.
#[derive(Debug)]
struct ChannelData<T> {
    data_ptr: *const T,
    /// The number of elements currently in the channel.
    ///
    /// This is also the main way of synchonizing between `Sender` and `Receiver`.
    len: AtomicUsize,
    capacity: usize,
    // Position of the sender, it points to the last written element.
    head: AtomicPtr<T>,
    // Position of the receiver, it points to the next readable element if the channel isn't empty.
    tail: AtomicPtr<T>,
}

#[derive(Debug)]
pub struct Sender<T> {
    buf: Arc<ChannelData<T>>,
    // TODO: How about storing the start or end of the current half here? Would allow for easy switching and operations
    //       could work relative to the stored pointer without caring whether its the second or first half.
    /// Points to the first element after the current half.
    current_half_end: *const T,
}

#[derive(Debug)]
pub struct Receiver<T> {
    buf: Arc<ChannelData<T>>,
}

impl<T> ChannelData<T> {
    #[inline(always)]
    fn half_point(&self) -> *const T {
        unsafe { self.data_ptr.add(self.capacity) }
    }
}

impl<T> Sender<T> {
    pub fn push(&mut self, value: T) {
        // Acquire ordering to make sure we get the correct number of elements.
        let len = self.buf.len.load(Ordering::Acquire);
        if len >= self.buf.capacity {
            todo!("handle blocking when pushing and capacity has been reached");
        }

        // Relaxed ordering since `head` is only accessed by `self`.
        let mut head = self.buf.head.load(Ordering::Relaxed);
        head = unsafe { head.add(1) };

        if head.cast_const() >= self.current_half_end {
            dbg!("at end of half");
            // next element has to be written into the second half.
            head = self.switch(len);
        }

        // Write the value and update head to point to it.
        unsafe { head.write(value) };
        self.buf.head.store(head, Ordering::Relaxed);

        // TODO: Could this be Release instead?
        self.buf.len.fetch_add(1, Ordering::AcqRel);
    }

    fn switch(&mut self, len: usize) -> *mut T {
        if self.current_half_end == self.buf.half_point() {
            self.current_half_end = unsafe { self.buf.half_point().add(self.buf.capacity) };
        } else {
            self.current_half_end = self.buf.half_point();
        }
        unsafe { self.current_half_end.sub(len) }.cast_mut()
    }
}
