// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Isolated test for reset_dead_letter_count().
//!
//! This test runs in its own binary (separate process) to avoid interfering
//! with other tests that use delta-based dead_letter_count() assertions.

#[test]
fn test_reset_dead_letter_count() {
    // Get current count
    let initial = rsactor::dead_letter_count();

    // Reset should work without error
    rsactor::reset_dead_letter_count();

    // After reset, count should be 0
    assert_eq!(
        rsactor::dead_letter_count(),
        0,
        "Dead letter count should be 0 after reset"
    );

    let _ = initial;
}
