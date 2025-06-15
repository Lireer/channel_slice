use std::{
    collections::VecDeque,
    ops::Range,
    sync::{Arc, Condvar, Mutex},
};

use crate::{SliceChannelReceiver, SliceChannelSender};

/// Shared state for the simple channel implementation
struct ChannelState<T> {
    /// The actual data storage
    data: VecDeque<T>,
    /// Maximum capacity of the channel
    capacity: usize,
    /// Whether the sender has been dropped
    sender_disconnected: bool,
}

/// Shared synchronization primitives
struct ChannelSync<T> {
    /// Shared state protected by mutex
    state: Mutex<ChannelState<T>>,
    /// Condition variable for notifying when data is available
    data_available: Condvar,
    /// Condition variable for notifying when space is available
    space_available: Condvar,
}

/// A simple slice wrapper that owns its data
pub struct OwnedSlice<T> {
    data: Vec<T>,
}

impl<T> AsRef<[T]> for OwnedSlice<T> {
    fn as_ref(&self) -> &[T] {
        &self.data
    }
}

impl<T> OwnedSlice<T> {
    fn new(data: Vec<T>) -> Self {
        Self { data }
    }
}

/// Simple sender implementation
pub struct SimpleSender<T> {
    shared: Arc<ChannelSync<T>>,
}

impl<T> SimpleSender<T> {
    /// Returns the current number of elements in the channel.
    pub fn len(&self) -> usize {
        let state = self.shared.state.lock().unwrap();
        state.data.len()
    }

    /// Returns the total capacity of the channel.
    pub fn capacity(&self) -> usize {
        let state = self.shared.state.lock().unwrap();
        state.capacity
    }

    /// Returns whether the channel is currently empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Simple receiver implementation
pub struct SimpleReceiver<T> {
    shared: Arc<ChannelSync<T>>,
}

impl<T> SimpleReceiver<T> {
    /// Returns the current number of elements in the channel.
    pub fn len(&self) -> usize {
        let state = self.shared.state.lock().unwrap();
        state.data.len()
    }

    /// Returns the total capacity of the channel.
    pub fn capacity(&self) -> usize {
        let state = self.shared.state.lock().unwrap();
        state.capacity
    }

    /// Returns whether the channel is currently empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Create a bounded channel with the specified capacity
pub fn create_simple_channel<T>(capacity: usize) -> (SimpleSender<T>, SimpleReceiver<T>) {
    assert!(capacity > 0, "capacity must be greater than 0");

    let shared = Arc::new(ChannelSync {
        state: Mutex::new(ChannelState {
            data: VecDeque::with_capacity(capacity),
            capacity,
            sender_disconnected: false,
        }),
        data_available: Condvar::new(),
        space_available: Condvar::new(),
    });

    let sender = SimpleSender {
        shared: shared.clone(),
    };

    let receiver = SimpleReceiver { shared };

    (sender, receiver)
}

impl<T> Drop for SimpleSender<T> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.sender_disconnected = true;
            // Wake up any waiting receivers
            self.shared.data_available.notify_all();
        }
        // If the mutex is poisoned, we can't update the state, but that's okay
        // because the receiver will eventually detect the disconnection through other means
    }
}

impl<T> SliceChannelSender<T> for SimpleSender<T> {
    fn append<D>(&mut self, data: D)
    where
        D: IntoIterator<Item = T>,
        D::IntoIter: ExactSizeIterator,
    {
        let iter = data.into_iter();
        let len = iter.len();

        if len == 0 {
            return;
        }

        let mut state = self.shared.state.lock().unwrap();

        // Panic if data is larger than capacity
        if len > state.capacity {
            panic!(
                "data contains more elements ({}) than channel capacity ({})",
                len, state.capacity
            );
        }

        // Wait until there's enough space
        while state.data.len() + len > state.capacity {
            state = self.shared.space_available.wait(state).unwrap();
        }

        // Add all elements
        state.data.extend(iter);

        // Notify waiting receivers
        self.shared.data_available.notify_all();
    }

    fn try_append<D>(&mut self, data: D) -> Result<(), usize>
    where
        D: IntoIterator<Item = T>,
        D::IntoIter: ExactSizeIterator,
    {
        let iter = data.into_iter();
        let len = iter.len();

        if len == 0 {
            return Ok(());
        }

        let mut state = self.shared.state.lock().unwrap();

        // Panic if data is larger than capacity
        if len > state.capacity {
            panic!(
                "data contains more elements ({}) than channel capacity ({})",
                len, state.capacity
            );
        }

        // Check if there's enough space
        let current_len = state.data.len();
        if current_len + len > state.capacity {
            return Err(len - (state.capacity - current_len));
        }

        // Add all elements
        state.data.extend(iter);

        // Notify waiting receivers
        self.shared.data_available.notify_all();

        Ok(())
    }
}

impl<T: Clone> SliceChannelReceiver<T> for SimpleReceiver<T> {
    type Slice<'a>
        = OwnedSlice<T>
    where
        Self: 'a;

    fn slice(&mut self, range: Range<usize>) -> Self::Slice<'_> {
        let end = range.end;

        let mut state = self.shared.state.lock().unwrap();

        // Panic if range end is greater than capacity
        if end > state.capacity {
            panic!(
                "range end ({}) is greater than capacity ({})",
                end, state.capacity
            );
        }

        // Wait until we have enough elements
        while state.data.len() < end {
            if state.sender_disconnected {
                panic!("sender disconnected while waiting for elements");
            }
            state = self.shared.data_available.wait(state).unwrap();
        }

        // Extract the slice data
        let slice_data: Vec<T> = state.data.range(range).cloned().collect();

        OwnedSlice::new(slice_data)
    }

    fn pop(&mut self, n: usize, buf: &mut Vec<T>) {
        let mut state = self.shared.state.lock().unwrap();

        // Panic if n is greater than capacity
        if n > state.capacity {
            panic!("n ({}) is greater than capacity ({})", n, state.capacity);
        }

        // Wait until we have enough elements
        while state.data.len() < n {
            if state.sender_disconnected {
                panic!("sender disconnected while waiting for elements");
            }
            state = self.shared.data_available.wait(state).unwrap();
        }

        // Pop n elements and add to buf
        for _ in 0..n {
            if let Some(item) = state.data.pop_front() {
                buf.push(item);
            }
        }

        // Notify waiting senders
        self.shared.space_available.notify_all();
    }

    fn consume_exact(&mut self, n: usize) {
        let mut state = self.shared.state.lock().unwrap();

        // Panic if n is greater than capacity
        if n > state.capacity {
            panic!("n ({}) is greater than capacity ({})", n, state.capacity);
        }

        // Wait until we have enough elements
        while state.data.len() < n {
            if state.sender_disconnected {
                panic!("sender disconnected while waiting for elements");
            }
            state = self.shared.data_available.wait(state).unwrap();
        }

        // Consume n elements
        for _ in 0..n {
            state.data.pop_front();
        }

        // Notify waiting senders
        self.shared.space_available.notify_all();
    }

    fn try_slice(&mut self, range: Range<usize>) -> Result<Self::Slice<'_>, usize> {
        let end = range.end;

        let state = self.shared.state.lock().unwrap();

        // Panic if range end is greater than capacity
        if end > state.capacity {
            panic!(
                "range end ({}) is greater than capacity ({})",
                end, state.capacity
            );
        }

        // Check if we have enough elements
        let current_len = state.data.len();
        if current_len < end {
            if state.sender_disconnected {
                panic!("sender disconnected while waiting for elements");
            }
            return Err(end - current_len);
        }

        // Extract the slice data
        let slice_data: Vec<T> = state.data.range(range).cloned().collect();

        Ok(OwnedSlice::new(slice_data))
    }

    fn try_pop(&mut self, n: usize, buf: &mut Vec<T>) -> Result<(), usize> {
        let mut state = self.shared.state.lock().unwrap();

        // Panic if n is greater than capacity
        if n > state.capacity {
            panic!("n ({}) is greater than capacity ({})", n, state.capacity);
        }

        // Check if we have enough elements
        if state.data.len() < n {
            return Err(n - state.data.len());
        }

        // Pop n elements and add to buf
        for _ in 0..n {
            if let Some(item) = state.data.pop_front() {
                buf.push(item);
            }
        }

        // Notify waiting senders
        self.shared.space_available.notify_all();

        Ok(())
    }

    fn try_consume_exact(&mut self, n: usize) -> Result<(), usize> {
        let mut state = self.shared.state.lock().unwrap();

        // Panic if n is greater than capacity
        if n > state.capacity {
            panic!("n ({}) is greater than capacity ({})", n, state.capacity);
        }

        // Check if we have enough elements
        if state.data.len() < n {
            return Err(n - state.data.len());
        }

        // Consume n elements
        for _ in 0..n {
            state.data.pop_front();
        }

        // Notify waiting senders
        self.shared.space_available.notify_all();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_send_receive() {
        let (mut sender, mut receiver) = create_simple_channel::<i32>(10);

        // Test basic append and consume
        sender.append(vec![1, 2, 3]);
        receiver.consume_exact(2);

        let slice = receiver.try_slice(0..1).unwrap();
        assert_eq!(slice.as_ref(), &[3]);
        assert_eq!(receiver.len(), 1);
        let mut out = Vec::new();
        receiver.pop(1, &mut out);
        assert_eq!(out, vec![3]);
        assert_eq!(receiver.len(), 0);
    }

    #[test]
    fn test_try_operations() {
        let (mut sender, mut receiver) = create_simple_channel::<i32>(5);

        // Try to receive from empty channel
        assert!(receiver.try_consume_exact(1).is_err());

        // Send some data
        sender.try_append(vec![1, 2, 3]).unwrap();

        // Try to receive
        let mut buf = Vec::new();
        receiver.try_pop(2, &mut buf).unwrap();
        assert_eq!(buf, vec![1, 2]);

        // Try to overfill
        let result = sender.try_append(vec![4, 5, 6, 7, 8]);
        assert!(result.is_err()); // Should not fit (1 element left + 5 new = 6 > 5 capacity)
    }

    #[test]
    fn test_slice_operations() {
        let (mut sender, mut receiver) = create_simple_channel::<i32>(10);

        sender.append(vec![0, 1, 2, 3, 4, 5]);

        let slice = receiver.try_slice(1..4).unwrap();
        assert_eq!(slice.as_ref(), &[1, 2, 3]);

        // Data should still be in the channel
        receiver.consume_exact(6);
    }

    #[test]
    #[should_panic(expected = "n (15) is greater than capacity (10)")]
    fn test_consume_more_than_capacity() {
        let (_, mut receiver) = create_simple_channel::<i32>(10);
        receiver.consume_exact(15);
    }

    #[test]
    #[should_panic(expected = "data contains more elements (15) than channel capacity (10)")]
    fn test_send_more_than_capacity() {
        let (mut sender, _) = create_simple_channel::<i32>(10);
        sender.append(0..15);
    }

    #[test]
    fn test_drop_sender_then_receiver() {
        let (sender, receiver) = create_simple_channel::<i32>(4);
        assert_eq!(sender.len(), 0);
        drop(sender);
        assert_eq!(receiver.len(), 0);
        drop(receiver);
    }

    #[test]
    fn test_drop_receiver_then_sender() {
        let (sender, receiver) = create_simple_channel::<i32>(4);
        assert_eq!(receiver.len(), 0);
        drop(receiver);
        assert_eq!(sender.len(), 0);
        drop(sender);
    }

    #[test]
    fn test_elements_dropped_correctly() {
        let (mut sender, mut receiver) = create_simple_channel(4);

        let elems: Vec<Arc<i32>> = [0, 1, 2, 3, 4, 5].into_iter().map(Arc::new).collect();

        sender.append(elems[0..4].iter().cloned());
        assert!(elems[0..4].iter().all(|e| Arc::strong_count(e) == 2)); // Use consume_exact to properly drop elements without keeping references
        receiver.consume_exact(2);
        sender.append(elems[4..].iter().cloned());
        assert!(elems[0..2].iter().all(|e| Arc::strong_count(e) == 1));
        assert!(elems[2..].iter().all(|e| Arc::strong_count(e) == 2));

        drop(receiver);
        assert!(elems[2..].iter().all(|e| Arc::strong_count(e) == 2));
        drop(sender);
        assert!(elems.iter().all(|e| Arc::strong_count(e) == 1));
    }

    #[test]
    fn test_send_and_receive_entire_buffer() {
        let (mut sender, mut receiver) = create_simple_channel(3);
        sender.append(vec![0, 1, 2]);
        let mut buf = Vec::new();
        receiver.pop(3, &mut buf);
        assert_eq!(buf, vec![0, 1, 2]);
        assert_eq!(receiver.len(), 0);
        assert_eq!(sender.len(), 0);
    }

    #[test]
    fn test_circular_buffer_behavior() {
        let (mut s, mut r) = create_simple_channel::<u32>(3);
        // Empty buffer: _ _ _
        s.append(vec![0]);
        // Buffer: 0 _ _
        s.append(vec![1]);
        // Buffer: 0 1 _
        s.append(vec![2]);
        // Buffer: 0 1 2   len: 3
        let mut buf = Vec::new();
        r.pop(1, &mut buf);
        assert_eq!(buf, vec![0]);
        // Buffer: _ 1 2   len: 2
        s.append(vec![3]);
        // Buffer: 3 1 2   len: 3
        buf.clear();
        r.pop(1, &mut buf);
        assert_eq!(buf, vec![1]);
        // Buffer: 3 _ 2   len: 2
        s.append(vec![4]);
        // Buffer: 3 4 2   len: 3
        buf.clear();
        r.pop(1, &mut buf);
        assert_eq!(buf, vec![2]);
        // Buffer: 3 4 _   len: 2
        buf.clear();
        r.pop(1, &mut buf);
        assert_eq!(buf, vec![3]);
    }

    #[test]
    fn test_try_send_into_full_buffer() {
        let (mut sender, _receiver) = create_simple_channel(2);

        // Fill the entire buffer
        sender.try_append(vec![0]).unwrap();
        sender.try_append(vec![1]).unwrap();
        assert_eq!(sender.len(), 2);

        // Test try_append with single element
        let err = sender.try_append(vec![3]);
        assert!(err.is_err());
        assert_eq!(sender.len(), 2);
    }

    #[test]
    fn test_try_send_vec_into_full_buffer() {
        let (mut sender, _receiver) = create_simple_channel(2);

        // Fill part of the buffer
        sender.try_append(vec![0]).unwrap();
        assert_eq!(sender.len(), 1);

        // Test try_append with vector that would overflow
        let vec = vec![2, 3];
        let err = sender.try_append(vec.clone());
        assert!(err.is_err());
        assert_eq!(sender.len(), 1);
    }

    #[test]
    fn test_capacity_and_len() {
        let (mut sender, mut receiver) = create_simple_channel::<i32>(5);

        assert_eq!(sender.capacity(), 5);
        assert_eq!(receiver.capacity(), 5);
        assert_eq!(sender.len(), 0);
        assert_eq!(receiver.len(), 0);

        sender.append(vec![1, 2, 3]);
        assert_eq!(sender.len(), 3);
        assert_eq!(receiver.len(), 3);

        let mut buf = Vec::new();
        receiver.pop(2, &mut buf);
        assert_eq!(sender.len(), 1);
        assert_eq!(receiver.len(), 1);
    }

    #[test]
    fn test_consume_exact_edge_cases() {
        let (mut sender, mut receiver) = create_simple_channel::<i32>(5);

        // Consume from empty channel
        receiver.consume_exact(0); // Should not panic

        sender.append(vec![1, 2, 3]);
        receiver.consume_exact(3);
        assert_eq!(receiver.len(), 0);
    }

    #[test]
    fn test_slice_range_operations() {
        let (mut sender, mut receiver) = create_simple_channel::<i32>(10);

        sender.append(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

        // Test different slice ranges
        let slice1 = receiver.try_slice(0..3).unwrap();
        assert_eq!(slice1.as_ref(), &[0, 1, 2]);

        let slice2 = receiver.try_slice(3..6).unwrap();
        assert_eq!(slice2.as_ref(), &[3, 4, 5]);

        let slice3 = receiver.try_slice(6..10).unwrap();
        assert_eq!(slice3.as_ref(), &[6, 7, 8, 9]);

        // Data should still be in the channel
        assert_eq!(receiver.len(), 10);
    }

    #[test]
    fn test_slice_with_wrap_around() {
        let (mut sender, mut receiver) = create_simple_channel::<i32>(5);

        // Fill the buffer
        sender.append(vec![0, 1, 2, 3, 4]);

        // Remove some from front
        let mut buf = Vec::new();
        receiver.pop(2, &mut buf);
        assert_eq!(buf, vec![0, 1]);

        // Add to back (this will wrap around)
        sender.append(vec![5, 6]);

        // Now buffer should contain [2, 3, 4, 5, 6] logically
        let slice = receiver.try_slice(0..5).unwrap();
        assert_eq!(slice.as_ref(), &[2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_blocking_operations_timeout() {
        use std::time::{Duration, Instant};

        let (mut sender, _receiver) = create_simple_channel::<i32>(2);

        // Fill the buffer
        sender.append(vec![1, 2]);

        // Try to send more with timeout - should fail quickly since buffer is full
        let start = Instant::now();
        let result = sender.try_append(vec![3]);
        let elapsed = start.elapsed();

        assert!(result.is_err());
        assert!(elapsed < Duration::from_millis(100)); // Should fail immediately
    }

    #[test]
    fn test_multi_element_operations() {
        let (mut sender, mut receiver) = create_simple_channel::<i32>(10);

        // Send multiple batches
        sender.append(vec![1, 2]);
        sender.append(vec![3, 4, 5]);
        sender.append(vec![6]);

        assert_eq!(sender.len(), 6);

        // Receive in different batch sizes
        let mut buf1 = Vec::new();
        receiver.pop(3, &mut buf1);
        assert_eq!(buf1, vec![1, 2, 3]);

        let mut buf2 = Vec::new();
        receiver.pop(2, &mut buf2);
        assert_eq!(buf2, vec![4, 5]);

        let mut buf3 = Vec::new();
        receiver.pop(1, &mut buf3);
        assert_eq!(buf3, vec![6]);

        assert_eq!(receiver.len(), 0);
    }
}
