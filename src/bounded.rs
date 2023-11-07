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
    /// Returned by [`Sender::try_*`][Sender::try_send] methods in cases in which their non-try
    /// counterpart would block.
    ///
    /// Contains the input value that could not be sent into the channel and the free capacity.
    WouldBlock(T, usize),
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
    ///
    /// If it's different from the end of the first half, the pointer will be set to `self.half_point()`.
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
        let len = dbg!(self.len());
        if len >= self.buf.capacity {
            return Err(SendError::WouldBlock(t, self.capacity() - len));
        }

        // SAFETY: We just checked, that `len` is lower than the allowed capacity.
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
        dbg!(self.buf.len.fetch_add(1, Ordering::AcqRel));
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
        let len = self.len();
        if len < n {
            return Err(RecvError::WouldBlock(len));
        }

        unsafe { Ok(self.peek_unchecked(n)) }
    }

    /// Peeks the next `n` elements in the channel.
    ///
    /// This method will *block* until enough elements become available or the [`Sender`] is disconnected.
    pub fn peek(&mut self, n: usize) -> Result<&[T], RecvError> {
        if n > self.capacity() {
            return Err(RecvError::OutOfBounds);
        }
        let len = self.len();
        if len < n {
            todo!("block peek until enough elements are available");
        }

        unsafe { Ok(self.peek_unchecked(n)) }
    }

    /// Peeks the next `n` elements without performing any checks.
    ///
    /// # SAFETY
    ///
    /// Channel must contain at least `n` elements.
    unsafe fn peek_unchecked(&mut self, n: usize) -> &[T] {
        // Check if we can get `n` elements from the current half, if not switch to the other half
        // which should then contain enough elements.
        let elems_in_half = self.offset_from_half_end();

        if n > elems_in_half {
            // Copy to other half and update tail accordingly.
            //
            // SAFETY: Switching is safe, since the check guarantees, that at least one element has
            //         been written into the other half.
            self.tail = unsafe { self.switch(elems_in_half) };
        }

        unsafe { std::slice::from_raw_parts(self.tail, n) }
    }

    pub fn try_recv(&mut self) -> Result<T, RecvError> {
        let len = dbg!(self.len());
        if len == 0 {
            return Err(RecvError::WouldBlock(len));
        }

        unsafe { Ok(self.recv_unchecked()) }
    }

    pub fn recv(&mut self) -> Result<T, RecvError> {
        let len = self.len();
        if len == 0 {
            todo!("block recv until an element is available");
        }

        unsafe { Ok(self.recv_unchecked()) }
    }

    /// Receive the next value without performing any length checks.
    ///
    /// # SAFETY
    ///
    /// Calling this method while the channel is empty is undefined behavior.
    unsafe fn recv_unchecked(&mut self) -> T {
        // Since `tail` always points to the next valid element

        // Read the value onto the stack
        let val: T = unsafe { std::ptr::read(self.tail) };

        // Forget the value in the buffer without dropping by updating the length
        let mut len = self.buf.len.fetch_sub(1, Ordering::AcqRel);
        assert!(len > 0);
        len -= 1;

        // Update tail, possibly to the other half.
        let elems_in_half = self.offset_from_half_end();
        if elems_in_half > 1 {
            // There are still more values left in this half
            self.tail = unsafe { self.tail.add(1) };
        } else {
            // At end of half, switch to the other half without having to copy any elements.
            self.buf.switch_half_ptr(&mut self.current_half_end);
            self.tail = match len {
                0 => {
                    // len is now 0, so nothing has been written into the other half yet, update to start of other half
                    unsafe { self.current_half_end.sub(self.buf.capacity).cast_mut() }
                }
                _ => {
                    // elements have already been written into the other half, update to `space_left_by_head`
                    unsafe {
                        self.current_half_end
                            .sub(
                                self.capacity()
                                    - self.buf.space_left_by_head.load(Ordering::Relaxed),
                            )
                            .cast_mut()
                    }
                }
            };
        }

        val
    }

    /// Removes `n` elements from the queue without blocking, dropping them if required.
    pub fn dequeue(&mut self, n: usize) -> Result<(), RecvError> {
        if n > self.capacity() {
            return Err(RecvError::OutOfBounds);
        }
        let len = self.len();
        if len > n {
            todo!("block dequeue until enough elements are in the queue");
        }

        unsafe { self.dequeue_unchecked(n) };
        Ok(())
    }

    /// Removes `n` elements from the queue without blocking, dropping them if required.
    pub fn try_dequeue(&mut self, n: usize) -> Result<(), RecvError> {
        if n > self.capacity() {
            return Err(RecvError::OutOfBounds);
        }
        let len = self.len();
        if n > len {
            return Err(RecvError::WouldBlock(len));
        }

        unsafe { self.dequeue_unchecked(n) };
        Ok(())
    }

    /// # SAFETY
    ///
    /// The caller must ensure `self.len()` is greater than or equal to `n`.
    unsafe fn dequeue_unchecked(&mut self, n: usize) {
        let elems_in_half = self.offset_from_half_end();

        let mut remaining = n;
        if remaining > elems_in_half {
            // TODO: Drop elements in this half ...

            // ... and switch halfs.
            // SAFETY: The check guarantees that at least one element has been written into the other half.
            self.tail = unsafe { self.switch(0) };

            remaining -= elems_in_half;
        }

        // TODO: Drop `remaining` elements in current half.

        // Update tail to reflect the removal.
        self.tail = unsafe { self.tail.add(remaining) };

        self.buf.len.fetch_sub(n, Ordering::AcqRel);
    }

    fn offset_from_half_end(&self) -> usize {
        assert!(self.current_half_end >= self.tail);
        unsafe { self.current_half_end.offset_from(self.tail) as usize }
    }

    /// # Safety
    ///
    /// `self.buf.space_left_by_head` must be in the other half, which means that the head must be
    /// in the half that the tail will be switched to.
    unsafe fn switch(&mut self, elems_in_half: usize) -> *mut T {
        self.buf.switch_half_ptr(&mut self.current_half_end);
        let space_left_by_head = self.buf.space_left_by_head.load(Ordering::Relaxed);
        assert!(space_left_by_head >= elems_in_half);

        let other_half_start = unsafe { self.current_half_end.sub(self.capacity()) };
        let new_tail: *mut T =
            unsafe { other_half_start.add(space_left_by_head - elems_in_half) }.cast_mut();
        unsafe { std::ptr::copy_nonoverlapping(self.tail, new_tail, elems_in_half) };
        new_tail
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::bounded::{create_bounded, SendError};

    use super::RecvError;

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
        assert_eq!(s.try_send(4), Err(SendError::WouldBlock(4, 0)));
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

    #[test]
    fn try_send_recv_multiple_capacities() {
        let capa = 5;
        let (mut s, mut r) = create_bounded(capa);

        let total_items = capa * 4;
        let sending_thread = std::thread::spawn(move || {
            let data = s.buf.data_ptr;
            for i in 0..total_items {
                // dbg!(&s);
                while s.try_send(i).is_err() {}
                println!("after send {i}: {:?}", unsafe {
                    std::slice::from_raw_parts(data, capa * 2)
                });
            }
        });

        let data = r.buf.data_ptr;
        for n in 0..total_items {
            loop {
                match r.try_recv() {
                    Ok(val) => {
                        // dbg!(val, n);
                        // dbg!(&r);
                        println!("after recv {n}: {:?}", unsafe {
                            std::slice::from_raw_parts(data, capa * 2)
                        });
                        assert_eq!(val, n);
                        break;
                    }
                    Err(RecvError::WouldBlock(0)) => continue,
                    Err(e) => panic!("unexpected error: {:?}", e),
                }
            }
        }

        sending_thread.join().unwrap();
    }

    #[test]
    fn send_till_full() {
        let (mut s, _r) = create_bounded(3);
        assert_eq!(s.try_send(0), Ok(()));
        assert_eq!(s.try_send(1), Ok(()));
        assert_eq!(s.try_send(2), Ok(()));
        assert_eq!(s.try_send(3), Err(SendError::WouldBlock(3, 0)));
        assert_eq!(s.try_send(4), Err(SendError::WouldBlock(4, 0)));
    }

    #[test]
    fn oh_no() {
        let (mut s, mut r) = create_bounded::<u32>(3);
        // _ _ _ | _ _ _
        assert_eq!(s.try_send(0), Ok(()));
        // 0 _ _ | _ _ _
        assert_eq!(s.try_send(1), Ok(()));
        // 0 1 _ | _ _ _
        assert_eq!(s.try_send(2), Ok(()));
        // 0 1 2 | _ _ _   len: 3
        assert_eq!(r.try_recv(), Ok(0));
        // _ 1 2 | _ _ _   len: 2
        assert_eq!(s.try_send(3), Ok(()));
        // _ 1 2 | _ _ 3   len: 3
        assert_eq!(r.try_recv(), Ok(1));
        // _ _ 2 | _ _ 3   len: 2
        assert_eq!(s.try_send(4), Ok(()));
        // ? ? 2 | _ _ 3   len: 3
        assert_eq!(r.try_recv(), Ok(2)); // <-- Panics because it receives 4 instead of 2 D:
    }

    #[test]
    fn single_thread_recv_one_by_one() {
        let capa = 5;
        let (mut s, mut r) = create_bounded(capa);
        let data = r.buf.data_ptr;

        for i in 0..(capa * 4) {
            assert_eq!(s.try_send(i), Ok(()));
            println!("after send {i}: {:?}", unsafe {
                std::slice::from_raw_parts(data, capa * 2)
            });
            assert_eq!(r.try_recv(), Ok(i));
            println!("after recv {i}: {:?}", unsafe {
                std::slice::from_raw_parts(data, capa * 2)
            });
        }
    }

    #[test]
    fn single_thread_dequeue_one_by_one() {
        let capa = 5;
        let (mut s, mut r) = create_bounded(capa);

        for i in 0..(capa * 4) {
            assert_eq!(s.try_send(i), Ok(()));
            assert_eq!(r.peek(1).unwrap(), &[i]);
            assert_eq!(r.dequeue(1), Ok(()));
        }
    }

    #[test]
    fn dequeue() {
        let capa = 5;
        let (mut s, mut r) = create_bounded(capa);

        let mut first_elem = 0;
        for i in 0..(capa * 10) {
            assert_eq!(s.try_send(i), Ok(()));
            if (i + 1) % 4 == 0 {
                assert_eq!(r.dequeue(4), Ok(()));
                first_elem += 4;
            } else {
                assert_eq!(r.try_peek(1).unwrap(), &[first_elem]);
            }
        }
    }
}
