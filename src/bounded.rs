use std::{
    alloc::Layout,
    mem,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

fn get_buffer_layout<T>(capacity: usize) -> Layout {
    Layout::from_size_align(2 * capacity * mem::size_of::<T>(), mem::align_of::<T>()).unwrap()
}

pub fn create_bounded<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let size_of_t = mem::size_of::<T>();
    assert_ne!(capacity, 0, "capacity is 0 but must be at least 1");
    assert_ne!(size_of_t, 0, "zero sized types are currenty not supported");
    assert!(
        !std::mem::needs_drop::<T>(),
        "types that need to be dropped are currently not supported"
    );

    let data_layout = get_buffer_layout::<T>(capacity);
    let data_start: *mut T = unsafe { std::alloc::alloc(data_layout) }.cast();
    if data_start.is_null() {
        // Abort if allocation failed, see `alloc` function for more information.
        std::alloc::handle_alloc_error(data_layout);
    }

    let channel = Arc::new(ChannelData {
        data_ptr: data_start,
        len: AtomicUsize::new(0),
        capacity,
        space_left_by_head: AtomicUsize::new(0),
    });

    let current_half_end = unsafe { data_start.add(capacity) };
    (
        Sender {
            buf: channel.clone(),
            head: data_start,
            current_half_end,
        },
        Receiver {
            buf: channel,
            tail: data_start,
            current_half_end,
        },
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
    capacity: usize,
    /// The number of elements currently in the channel.
    ///
    /// This is also the main way of synchonizing between `Sender` and `Receiver`.
    len: AtomicUsize,
    /// The memory in number of elements left empty when switching the head to the other half.
    ///
    /// This is the length at the time of the switch, i.e. the maximum number of elements the reader
    /// could copy after the switch.
    // Bikeshed name: space_left_by_head, head_write_offset, len_at_head_switch, len_at_switch
    //                write_start_in_half, write_start_after_switch
    space_left_by_head: AtomicUsize,
}

#[derive(Debug)]
pub struct Sender<T> {
    buf: Arc<ChannelData<T>>,
    /// Position of the sender, it points to the next unwritten element for the current half..
    ///
    /// If the last valid element in a half has been written, head will be updated to the first element after the half.
    head: *mut T,
    /// Points to the first element after the current half.
    current_half_end: *const T,
}

#[derive(Debug)]
pub struct Receiver<T> {
    buf: Arc<ChannelData<T>>,
    /// Position of the receiver, it points to the next readable element if the channel isn't empty.
    tail: *mut T,
    /// Points to the first element after the current half.
    current_half_end: *const T,
}

impl<T> ChannelData<T> {
    #[inline(always)]
    fn half_point(&self) -> *const T {
        unsafe { self.data_ptr.add(self.capacity) }
    }

    /// Makes `end_of_half` point to the end of the other half.
    fn switch_half_ptr(&self, end_of_half: &mut *const T) {
        if *end_of_half == self.half_point() {
            // Currently in the first half, switch to the second.
            *end_of_half = unsafe { self.half_point().add(self.capacity) };
        } else {
            // Currently in the second half, switch to the first.
            *end_of_half = self.half_point();
        }
    }
}

unsafe impl<T> Send for ChannelData<T> where T: Send {}
unsafe impl<T> Sync for ChannelData<T> where T: Sync {}

impl<T> Drop for ChannelData<T> {
    fn drop(&mut self) {
        let layout = get_buffer_layout::<T>(self.capacity);
        unsafe { std::alloc::dealloc(self.data_ptr as *mut u8, layout) };
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
        let mut head = self.head;

        if head.cast_const() >= self.current_half_end {
            // next element has to be written into the second half.

            // Update `space_left_by_head` using `Relaxed` ordering because the receiver may only
            // read it after `len` has been updated which only happens at the end and uses `AcqRel`.
            head = self.switch(len, Ordering::Relaxed);
        }

        // Write the value and update head to point to the next element.
        unsafe { head.write(value) };
        self.head = unsafe { head.add(1) };

        // TODO: Could this be Release instead?
        self.buf.len.fetch_add(1, Ordering::AcqRel);
    }

    /// Switch the head to the other half, leaving `len` elements of space at the start of the half.
    ///
    /// Updates `buf.space_left_by_head` to `len`.
    fn switch(&mut self, len: usize, order: Ordering) -> *mut T {
        let buf = &self.buf;

        buf.switch_half_ptr(&mut self.current_half_end);

        // Leave `len` elements empty before the next element, so the reader could potentially copy
        // all elements to this half.
        let new_head = unsafe { self.current_half_end.sub(buf.capacity - len) }.cast_mut();
        self.buf.space_left_by_head.store(len, order);
        new_head
    }
}

impl<T> Receiver<T> {
    pub fn peek(&mut self, n: usize) -> Option<&[T]> {
        let len = self.buf.len.load(Ordering::Acquire);
        if len < n {
            return None;
        }

        // Check if we can get `n` elements from the current half, if not switch to the other half
        // which should then contain enough elements.
        let elems_in_half = self.offset_from_half_end();

        if n > elems_in_half {
            // Copy to other half and update tail accordingly.
            self.tail = self.switch(elems_in_half);
        }

        Some(unsafe { std::slice::from_raw_parts(self.tail, n) })
    }

    fn offset_from_half_end(&self) -> usize {
        assert!(self.current_half_end >= self.tail);
        unsafe { self.current_half_end.offset_from(self.tail) as usize }
    }

    fn switch(&mut self, elems_in_half: usize) -> *mut T {
        self.buf.switch_half_ptr(&mut self.current_half_end);
        let space_left_by_head = self.buf.space_left_by_head.load(Ordering::Relaxed);
        assert!(space_left_by_head >= elems_in_half);

        let new_tail: *mut T =
            unsafe { self.buf.data_ptr.add(space_left_by_head - elems_in_half) }.cast_mut();
        unsafe { std::ptr::copy_nonoverlapping(self.tail, new_tail, elems_in_half) };
        new_tail
    }
}

impl Sender<f32> {
    #[inline(never)]
    pub fn push_f32(&mut self, val: f32) {
        self.push(val)
    }

    #[inline(never)]
    pub fn switch_f32(&mut self, len: usize) -> *mut f32 {
        self.switch(len, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::create_bounded;

    #[test]
    fn send_elements() {
        let (mut s, mut r) = create_bounded(3);
        for i in 0..3 {
            s.push(i);
        }
        assert_eq!(Some(vec![0, 1, 2].as_slice()), r.peek(3));
    }
}
