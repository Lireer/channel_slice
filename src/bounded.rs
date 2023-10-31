use std::{
    alloc::Layout,
    mem,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecvError {
    /// Returned by [`Receiver::try_*`][Receiver::try_recv] methods in cases in which their non-try
    /// counterpart would block.
    ///
    /// Contains the current number of elements in the channel.
    WouldBlock(usize),
    OutOfBounds,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError<T> {
    Full(T),
    Disconnected(T),
}

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

unsafe impl<T> Send for Sender<T> where T: Send {}
unsafe impl<T> Sync for Sender<T> where T: Sync {}

unsafe impl<T> Send for Receiver<T> where T: Send {}
unsafe impl<T> Sync for Receiver<T> where T: Sync {}

impl<T> Drop for ChannelData<T> {
    fn drop(&mut self) {
        let layout = get_buffer_layout::<T>(self.capacity);
        unsafe { std::alloc::dealloc(self.data_ptr as *mut u8, layout) };
    }
}

impl<T> Sender<T> {
    /// Returns the current number of elements in the channel.
    pub fn len(&self) -> usize {
        // Acquire ordering to make sure we get the correct number of elements.
        self.buf.len.load(Ordering::Acquire)
    }

    /// Returns whether the channel is currently empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the total number of elements the channel can hold at a time.
    pub fn capacity(&self) -> usize {
        self.buf.capacity
    }

    /// Sends a value on this channel, blocking if the channel is full.
    pub fn send(&mut self, t: T) -> Result<(), SendError<T>> {
        // TODO: Check if the receiver has disconnected and return SendError::Disconnected, if so.
        let len = self.len();
        if len >= self.buf.capacity {
            todo!("Impl blocking send");
        }

        // SAFETY: We just checked, that at `len` is lower than the allowed capacity.
        unsafe { self.send_unchecked(t, len) };
        Ok(())
    }

    /// Attempts to send a value on this channel
    pub fn try_send(&mut self, t: T) -> Result<(), SendError<T>> {
        // TODO: Check if the receiver has disconnected and return SendError::Disconnected, if so.
        let len = self.len();
        if len >= self.buf.capacity {
            return Err(SendError::Full(t));
        }

        // SAFETY: We just checked, that at `len` is lower than the allowed capacity.
        unsafe { self.send_unchecked(t, len) };
        Ok(())
    }

    /// Switch the head to the other half, leaving `len` elements of space at the start of the half.
    ///
    /// Updates `buf.space_left_by_head` to `len`, for which the [`store`][AtomicUsize::store] uses
    /// `order`.
    fn switch(&mut self, len: usize, order: Ordering) -> *mut T {
        let buf = &self.buf;

        buf.switch_half_ptr(&mut self.current_half_end);

        // Leave `len` elements empty before the next element, so the reader could potentially copy
        // all elements to this half.
        let new_head = unsafe { self.current_half_end.sub(buf.capacity - len) }.cast_mut();
        self.buf.space_left_by_head.store(len, order);
        new_head
    }

    /// Send a value into the channel without checking if `len + 1` would be bigger than the capacity.
    ///
    /// The `len` argument is required to avoid calling `self.len` twice.
    unsafe fn send_unchecked(&mut self, t: T, len: usize) {
        if self.head.cast_const() >= self.current_half_end {
            // element has to be written into the other half.

            // Update `space_left_by_head` using `Relaxed` ordering because the receiver may only
            // read it after `len` has been updated which only happens at the end and uses `AcqRel`.
            // TODO: Check if this is actually enough given the weak guarantees around Relaxed
            // ordering, i.e. what about reordering of statements on the same thread?
            self.head = self.switch(len, Ordering::Relaxed);
        }

        // Write the value and update head to point to the next element.
        unsafe { self.head.write(t) };
        self.head = unsafe { self.head.add(1) };

        // TODO: Could this be Release instead?
        self.buf.len.fetch_add(1, Ordering::AcqRel);
    }
}

impl<T> Receiver<T> {
    /// Returns the current number of elements in the channel.
    pub fn len(&self) -> usize {
        // Acquire ordering to make sure we get the correct number of elements.
        self.buf.len.load(Ordering::Acquire)
    }

    /// Returns whether the channel is currently empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the total number of elements the channel can hold at a time.
    pub fn capacity(&self) -> usize {
        self.buf.capacity
    }

    pub fn try_peek(&mut self, n: usize) -> Result<&[T], RecvError> {
        if n > self.capacity() {
            return Err(RecvError::OutOfBounds);
        }
        let len = self.buf.len.load(Ordering::Acquire);
        if len < n {
            return Err(RecvError::WouldBlock(len));
        }

        // Check if we can get `n` elements from the current half, if not switch to the other half
        // which should then contain enough elements.
        let elems_in_half = self.offset_from_half_end();

        if n > elems_in_half {
            // Copy to other half and update tail accordingly.
            self.tail = self.switch(elems_in_half);
        }

        Ok(unsafe { std::slice::from_raw_parts(self.tail, n) })
    }

    pub fn peek(&mut self, n: usize) -> Result<&[T], RecvError> {
        todo!()
    }

    pub fn try_recv(&mut self) -> Result<T, RecvError> {
        todo!()
    }

    pub fn recv(&mut self) -> Result<T, RecvError> {
        todo!()
    }

    /// Removes `n` elements from the queue, dropping them if required.
    pub fn deque(&mut self, n: usize) -> Result<(), RecvError> {
        todo!()
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::bounded::{create_bounded, SendError};

    #[test]
    #[should_panic(expected = "capacity is 0 but must be at least 1")]
    fn empty_channel() {
        create_bounded::<u32>(0);
    }

    #[test]
    #[should_panic(expected = "types that need to be dropped are currently not supported")]
    fn droppable_type_channel() {
        create_bounded::<String>(1);
    }

    #[test]
    #[should_panic(expected = "zero sized types are currenty not supported")]
    fn zst_channel() {
        struct Zst;
        create_bounded::<Zst>(2);
    }

    #[test]
    fn non_copy_type_channel() {
        let (mut s, mut r) = create_bounded::<AtomicBool>(1);
        s.send(AtomicBool::new(false)).unwrap();

        let b = r.try_peek(1).unwrap();
        b[0].store(true, Ordering::Relaxed);
        // let atomic = r.recv(1).unwrap();
    }

    #[test]
    fn send_elements() {
        let (mut s, mut r) = create_bounded(3);
        for i in 0..3 {
            s.try_send(i).unwrap();
        }
        assert_eq!(Ok(vec![0, 1, 2].as_slice()), r.try_peek(3));
    }

    #[test]
    fn nonblocking_when_full() {
        let (mut s, _r) = create_bounded(3);
        for i in 0..3 {
            s.try_send(i).unwrap();
        }
        assert_eq!(s.try_send(4), Err(SendError::Full(4)));
    }

    #[test]
    fn send_recv_multiple_capacities() {
        let capa = 5;
        let (mut s, mut r) = create_bounded(capa);

        let total_items = capa * 4;
        let sending_thread = std::thread::spawn(move || {
            for i in 0..total_items {
                while s.try_send(i).is_err() {}
            }
        });

        for n in 0..total_items {
            assert_eq!(r.recv(), Ok(n));
        }

        sending_thread.join().unwrap();
    }
}
