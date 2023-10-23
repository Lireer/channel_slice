use std::{
    alloc::Layout,
    sync::{atomic::AtomicPtr, Arc},
};

/// A bounded peekable channel.
///
/// # Capacity and Allocation
///
/// The capacity describes the maximum number of elements the channel can hold at a time. The amount
/// of allocated memory will almost be twice the amount required by `capacity` elements.
struct ChannelData<T> {
    data_ptr: *const T,
    capacity: usize,
    // Position of the sender
    head: AtomicPtr<T>,
    // Position of the receiver
    tail: AtomicPtr<T>,
}

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
    let data_start = unsafe { std::alloc::alloc(data_layout) };
    if data_start.is_null() {
        // Abort if allocation failed, see `alloc` function for more information.
        std::alloc::handle_alloc_error(data_layout);
    }

    let channel = ChannelData {
        data_ptr: AtomicPtr::new(data_start),
        capacity,
        head: AtomicPtr::new(data_start),
        tail: AtomicPtr::new(data_start),
    }
}

pub struct Sender<T> {
    buf: Arc<ChannelData<T>>,
}

pub struct Receiver<T> {
    buf: Arc<ChannelData<T>>,
}
