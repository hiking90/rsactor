// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use super::MetricsSnapshot;

/// Per-actor metrics storage using lock-free atomic operations.
///
/// This collector is designed for high-performance metrics gathering with minimal
/// contention. All operations use `Ordering::Relaxed` which provides eventual
/// consistency - suitable for monitoring where exact ordering is not critical.
///
/// # Thread Safety
///
/// The collector is safe to use from multiple threads concurrently:
/// - Writes (recording) use atomic fetch-and-add operations
/// - Reads (snapshot) load atomic values without locking
///
/// # Overflow Protection
///
/// - `total_processing_nanos` uses saturating addition to prevent wraparound
/// - Individual durations are capped at `u64::MAX` nanoseconds (~584 years)
#[derive(Debug)]
pub(crate) struct MetricsCollector {
    /// Number of messages processed
    message_count: AtomicU64,
    /// Number of errors during message handling
    error_count: AtomicU64,
    /// Cumulative processing time in nanoseconds (saturating)
    total_processing_nanos: AtomicU64,
    /// Maximum processing time observed in nanoseconds
    max_processing_nanos: AtomicU64,
    /// Last activity timestamp as milliseconds since UNIX_EPOCH
    last_activity_millis: AtomicU64,
    /// When the actor started (for uptime calculation)
    start_instant: Instant,
}

impl MetricsCollector {
    /// Creates a new metrics collector with all counters at zero.
    pub fn new() -> Self {
        Self {
            message_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_processing_nanos: AtomicU64::new(0),
            max_processing_nanos: AtomicU64::new(0),
            last_activity_millis: AtomicU64::new(0),
            start_instant: Instant::now(),
        }
    }

    /// Records a completed message processing with its duration.
    ///
    /// This method:
    /// 1. Increments the message count
    /// 2. Adds the duration to total processing time (saturating)
    /// 3. Updates max processing time if this was slower
    /// 4. Updates the last activity timestamp
    #[inline]
    pub fn record_message(&self, duration: Duration) {
        self.message_count.fetch_add(1, Ordering::Relaxed);

        // Safe conversion: cap at u64::MAX (~584 years in nanos)
        let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;

        // Saturating add to prevent wraparound
        let _ = self.total_processing_nanos.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| Some(current.saturating_add(nanos)),
        );

        // Update max using atomic fetch_max
        self.max_processing_nanos
            .fetch_max(nanos, Ordering::Relaxed);

        self.update_last_activity();
    }

    /// Records an error during message handling.
    ///
    /// Reserved for future error tracking integration.
    #[inline]
    #[allow(dead_code)]
    pub fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Updates the last activity timestamp to now.
    fn update_last_activity(&self) {
        let millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis()
            .min(u64::MAX as u128) as u64;
        self.last_activity_millis.store(millis, Ordering::Relaxed);
    }

    /// Converts the last activity timestamp to SystemTime.
    fn get_last_activity(&self) -> Option<SystemTime> {
        let millis = self.last_activity_millis.load(Ordering::Relaxed);
        if millis == 0 {
            None
        } else {
            SystemTime::UNIX_EPOCH.checked_add(Duration::from_millis(millis))
        }
    }

    /// Creates an immutable snapshot of current metrics.
    ///
    /// The snapshot is consistent for each individual field but may not
    /// represent a single point in time across all fields when read
    /// concurrently with message processing.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let count = self.message_count.load(Ordering::Relaxed);
        let total_nanos = self.total_processing_nanos.load(Ordering::Relaxed);

        MetricsSnapshot {
            message_count: count,
            avg_processing_time: if count > 0 {
                Duration::from_nanos(total_nanos / count)
            } else {
                Duration::ZERO
            },
            max_processing_time: Duration::from_nanos(
                self.max_processing_nanos.load(Ordering::Relaxed),
            ),
            error_count: self.error_count.load(Ordering::Relaxed),
            uptime: self.start_instant.elapsed(),
            last_activity: self.get_last_activity(),
        }
    }

    // Direct accessor methods for convenience

    /// Returns the total number of messages processed.
    #[inline]
    pub fn message_count(&self) -> u64 {
        self.message_count.load(Ordering::Relaxed)
    }

    /// Returns the total number of errors recorded.
    #[inline]
    pub fn error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }

    /// Returns the average processing time per message.
    #[inline]
    pub fn avg_processing_time(&self) -> Duration {
        let count = self.message_count.load(Ordering::Relaxed);
        if count > 0 {
            let total_nanos = self.total_processing_nanos.load(Ordering::Relaxed);
            Duration::from_nanos(total_nanos / count)
        } else {
            Duration::ZERO
        }
    }

    /// Returns the maximum processing time observed.
    #[inline]
    pub fn max_processing_time(&self) -> Duration {
        Duration::from_nanos(self.max_processing_nanos.load(Ordering::Relaxed))
    }

    /// Returns the uptime since actor start.
    #[inline]
    pub fn uptime(&self) -> Duration {
        self.start_instant.elapsed()
    }

    /// Returns the last activity timestamp.
    #[inline]
    pub fn last_activity(&self) -> Option<SystemTime> {
        self.get_last_activity()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for measuring message processing time.
///
/// When this guard is dropped, it automatically records the elapsed time
/// to the associated `MetricsCollector`.
///
/// # Example
///
/// ```rust,ignore
/// let guard = MessageProcessingGuard::new(&collector);
/// // ... process message ...
/// drop(guard); // Automatically records duration
/// ```
pub(crate) struct MessageProcessingGuard<'a> {
    collector: &'a MetricsCollector,
    start: Instant,
}

impl<'a> MessageProcessingGuard<'a> {
    /// Creates a new guard that will record to the given collector.
    #[inline]
    pub fn new(collector: &'a MetricsCollector) -> Self {
        Self {
            collector,
            start: Instant::now(),
        }
    }
}

impl Drop for MessageProcessingGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        self.collector.record_message(self.start.elapsed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_initial_state() {
        let collector = MetricsCollector::new();
        let snapshot = collector.snapshot();

        assert_eq!(snapshot.message_count, 0);
        assert_eq!(snapshot.error_count, 0);
        assert_eq!(snapshot.avg_processing_time, Duration::ZERO);
        assert_eq!(snapshot.max_processing_time, Duration::ZERO);
        assert!(snapshot.last_activity.is_none());
    }

    #[test]
    fn test_record_message() {
        let collector = MetricsCollector::new();

        collector.record_message(Duration::from_millis(100));
        collector.record_message(Duration::from_millis(200));

        assert_eq!(collector.message_count(), 2);
        assert_eq!(collector.avg_processing_time(), Duration::from_millis(150));
        assert_eq!(collector.max_processing_time(), Duration::from_millis(200));
        assert!(collector.last_activity().is_some());
    }

    #[test]
    fn test_record_error() {
        let collector = MetricsCollector::new();

        collector.record_error();
        collector.record_error();

        assert_eq!(collector.error_count(), 2);
    }

    #[test]
    fn test_guard_records_duration() {
        let collector = MetricsCollector::new();

        {
            let _guard = MessageProcessingGuard::new(&collector);
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(collector.message_count(), 1);
        assert!(collector.max_processing_time() >= Duration::from_millis(10));
    }

    #[test]
    fn test_uptime_increases() {
        let collector = MetricsCollector::new();
        let uptime1 = collector.uptime();
        std::thread::sleep(Duration::from_millis(10));
        let uptime2 = collector.uptime();

        assert!(uptime2 > uptime1);
    }
}
