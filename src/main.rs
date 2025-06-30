use slicebuf::lock_free_ringbuf;

fn main() {
    let n = 1000;

    let (mut sender, mut reader) = lock_free_ringbuf::create_bounded(n);

    for i in 0..n {
        while let Err(_) = sender.try_send(i) {}
    }

    for _ in 0..(n / 100) {
        let len = 100;
        assert_eq!(reader.recv_up_to(len).len(), len);
    }
}
