use std::sync::atomic::{AtomicU16, Ordering};

use bevy_remote::http::DEFAULT_PORT;

static NEXT_TEST_PORT: AtomicU16 = AtomicU16::new(DEFAULT_PORT + 1);

// Prevent conflicts between two different tests
pub fn next_test_port() -> u16 {
    NEXT_TEST_PORT.fetch_add(1, Ordering::Relaxed)
}
