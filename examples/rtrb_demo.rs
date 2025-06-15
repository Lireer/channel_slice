// Example demonstrating the rtrb-based SliceChannel implementation
use slicebuf::rtrb::create_bounded;
use slicebuf::{SliceChannelReceiver, SliceChannelSender};

fn main() {
    println!("RTRB-based SliceChannel Example");
    println!("===============================");

    // Create a bounded channel with capacity 10
    let (mut sender, mut receiver) = create_bounded::<i32>(10);

    println!("Created channel with capacity 10");

    // Test basic send/receive operations
    println!("\n1. Basic Operations:");

    // Send some data using the SliceChannelSender trait
    sender.append(vec![1, 2, 3, 4, 5]);
    println!("Sent: [1, 2, 3, 4, 5]");
    println!("Available items in receiver: {}", receiver.slots());

    // Receive data using pop_slice
    let mut buffer = Vec::new();
    let received = receiver.pop(3, &mut buffer);
    println!("Received via pop_slice(3): {:?}", received);
    println!("Remaining items in receiver: {}", receiver.slots());

    // Try non-blocking operations
    println!("\n2. Non-blocking Operations:");

    // Try to send more data
    match sender.try_append(vec![6, 7, 8, 9, 10, 11]) {
        Ok(()) => println!("Successfully sent [6, 7, 8, 9, 10, 11]"),
        Err(needed) => println!("Failed to send, need {} more slots", needed),
    }

    // Try to receive more than available
    let mut buf = Vec::new();
    match receiver.try_pop(5, &mut buf) {
        Ok(()) => println!("Successfully received 5 items: {:?}", buf),
        Err(needed) => println!("Failed to receive 5 items, need {} more", needed),
    }

    // Test slice viewing (with current implementation limitations)
    println!("\n3. Slice Operations:");

    // Add more data for slice testing
    sender.append(vec![20, 21, 22, 23, 24]);
    println!("Added more data: [20, 21, 22, 23, 24]");

    // Try to view a slice
    match receiver.try_slice(0..3) {
        Ok(slice) => {
            println!("Slice view [0..=2]: {:?}", slice.as_ref());
        }
        Err(needed) => {
            println!("Cannot view slice, need {} more items", needed);
        }
    }

    // Consume some data
    println!("\n4. Consume Operations:");
    receiver.consume_exact(2);
    println!("Consumed 2 items");
    println!("Remaining items: {}", receiver.slots());

    // Test error conditions
    println!("\n5. Error Handling:");

    // Try to consume more than available
    match receiver.try_consume_exact(10) {
        Ok(()) => println!("Successfully consumed 10 items"),
        Err(needed) => println!("Cannot consume 10 items, need {} more", needed),
    }

    // Test channel state
    println!("\n6. Channel State:");
    println!("Sender available slots: {}", sender.slots());
    println!("Receiver available items: {}", receiver.slots());
    println!("Sender is full: {}", sender.is_full());
    println!("Receiver is empty: {}", receiver.is_empty());

    println!("\nExample completed successfully!");
}
