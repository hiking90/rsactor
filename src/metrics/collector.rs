// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
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
/// - Individual durations are capped at `u64::MAX` nanoseconds (~584 years)
/// - The `total_*_nanos` counters (regular, priority, idle) accumulate with
///   plain atomic add; a summed total wraps only after ~584 years of
///   processing time
#[derive(Debug)]
pub(crate) struct MetricsCollector {
    /// Number of messages processed (regular mailbox only — priority messages are
    /// counted separately so callers can detect priority-channel abuse).
    message_count: AtomicU64,
    /// Cumulative processing time for regular messages, in nanoseconds
    total_processing_nanos: AtomicU64,
    /// Maximum regular message processing time observed in nanoseconds
    max_processing_nanos: AtomicU64,
    /// Number of priority messages processed (regular + drained-on-stop).
    priority_message_count: AtomicU64,
    /// Cumulative processing time for priority messages, in nanoseconds
    total_priority_processing_nanos: AtomicU64,
    /// Maximum priority message processing time observed in nanoseconds
    max_priority_processing_nanos: AtomicU64,
    /// Number of idle events dispatched to `on_idle` (successful returns only).
    idle_event_count: AtomicU64,
    /// Cumulative `on_idle` processing time, in nanoseconds
    total_idle_processing_nanos: AtomicU64,
    /// Maximum `on_idle` processing time observed in nanoseconds
    max_idle_processing_nanos: AtomicU64,
    /// Last activity timestamp as milliseconds since UNIX_EPOCH
    last_activity_millis: AtomicU64,
    /// Collector construction time. Used as the uptime baseline until the actor
    /// finishes `on_start` (see `started_at`).
    start_instant: Instant,
    /// When the actor's `on_start` completed successfully. Set once via
    /// [`mark_started`](Self::mark_started); until then `uptime` falls back to
    /// `start_instant`.
    started_at: OnceLock<Instant>,
}

impl MetricsCollector {
    /// Creates a new metrics collector with all counters at zero.
    pub fn new() -> Self {
        Self {
            message_count: AtomicU64::new(0),
            total_processing_nanos: AtomicU64::new(0),
            max_processing_nanos: AtomicU64::new(0),
            priority_message_count: AtomicU64::new(0),
            total_priority_processing_nanos: AtomicU64::new(0),
            max_priority_processing_nanos: AtomicU64::new(0),
            idle_event_count: AtomicU64::new(0),
            total_idle_processing_nanos: AtomicU64::new(0),
            max_idle_processing_nanos: AtomicU64::new(0),
            last_activity_millis: AtomicU64::new(0),
            start_instant: Instant::now(),
            started_at: OnceLock::new(),
        }
    }

    /// Marks the moment the actor's `on_start` completed successfully, which
    /// becomes the baseline for [`uptime`](Self::uptime). Idempotent — only the
    /// first call takes effect.
    #[inline]
    pub fn mark_started(&self) {
        let _ = self.started_at.set(Instant::now());
    }

    /// Returns the instant uptime is measured from: the `on_start`-completion
    /// time once recorded, otherwise the collector's construction time.
    #[inline]
    fn uptime_baseline(&self) -> Instant {
        self.started_at.get().copied().unwrap_or(self.start_instant)
    }

    /// Records a completed message processing with its duration.
    ///
    /// This method:
    /// 1. Increments the message count
    /// 2. Adds the duration to total processing time
    /// 3. Updates max processing time if this was slower
    /// 4. Updates the last activity timestamp
    #[inline]
    pub fn record_message(&self, duration: Duration) {
        self.message_count.fetch_add(1, Ordering::Relaxed);

        // Safe conversion: cap at u64::MAX (~584 years in nanos)
        let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;

        // Plain atomic add (single RMW) rather than a fetch_update CAS loop: the
        // accumulated total only wraps after ~584 years of summed processing
        // time, so saturating is unnecessary on this hot path.
        self.total_processing_nanos
            .fetch_add(nanos, Ordering::Relaxed);

        // Update max using atomic fetch_max
        self.max_processing_nanos
            .fetch_max(nanos, Ordering::Relaxed);

        self.update_last_activity();
    }

    /// Records a completed priority message processing with its duration.
    ///
    /// Priority messages are tracked in dedicated counters separate from regular
    /// messages so callers can detect imbalance (e.g. starvation) by comparing the
    /// two counts.
    #[inline]
    pub fn record_priority_message(&self, duration: Duration) {
        self.priority_message_count.fetch_add(1, Ordering::Relaxed);

        let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;

        // Plain atomic add (single RMW) instead of a fetch_update CAS loop; see
        // `record_message` for the wraparound rationale.
        self.total_priority_processing_nanos
            .fetch_add(nanos, Ordering::Relaxed);

        self.max_priority_processing_nanos
            .fetch_max(nanos, Ordering::Relaxed);

        self.update_last_activity();
    }

    /// Records a completed `on_idle` dispatch with its duration.
    ///
    /// Idle events do work with the same exclusive access to actor state as
    /// message handlers; without these counters an actor driven purely by
    /// subscribed idle streams would look permanently inactive
    /// (`last_activity == None`) and a slow `on_idle` would be invisible to
    /// the processing-time metrics.
    #[inline]
    pub fn record_idle_event(&self, duration: Duration) {
        self.idle_event_count.fetch_add(1, Ordering::Relaxed);

        let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;

        // Plain atomic add (single RMW) instead of a fetch_update CAS loop; see
        // `record_message` for the wraparound rationale.
        self.total_idle_processing_nanos
            .fetch_add(nanos, Ordering::Relaxed);

        self.max_idle_processing_nanos
            .fetch_max(nanos, Ordering::Relaxed);

        self.update_last_activity();
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
    /// Each individual atomic is read consistently, but the snapshot as a
    /// whole is not a single point in time: a recording that races this read
    /// can land between the loads, so *derived* values (the averages, computed
    /// from two separately-loaded atomics) can transiently disagree with the
    /// other fields. The averages are clamped to their corresponding maxima so
    /// the `avg <= max` invariant always holds in the values this returns; the
    /// residual skew is bounded by a single message's duration.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let count = self.message_count.load(Ordering::Relaxed);
        let total_nanos = self.total_processing_nanos.load(Ordering::Relaxed);
        let priority_count = self.priority_message_count.load(Ordering::Relaxed);
        let total_priority_nanos = self.total_priority_processing_nanos.load(Ordering::Relaxed);
        let idle_count = self.idle_event_count.load(Ordering::Relaxed);
        let total_idle_nanos = self.total_idle_processing_nanos.load(Ordering::Relaxed);
        let max_processing_time =
            Duration::from_nanos(self.max_processing_nanos.load(Ordering::Relaxed));
        let max_priority_processing_time =
            Duration::from_nanos(self.max_priority_processing_nanos.load(Ordering::Relaxed));
        let max_idle_processing_time =
            Duration::from_nanos(self.max_idle_processing_nanos.load(Ordering::Relaxed));

        MetricsSnapshot {
            message_count: count,
            avg_processing_time: total_nanos
                .checked_div(count)
                .map(Duration::from_nanos)
                .unwrap_or(Duration::ZERO)
                .min(max_processing_time),
            max_processing_time,
            priority_message_count: priority_count,
            avg_priority_processing_time: total_priority_nanos
                .checked_div(priority_count)
                .map(Duration::from_nanos)
                .unwrap_or(Duration::ZERO)
                .min(max_priority_processing_time),
            max_priority_processing_time,
            idle_event_count: idle_count,
            avg_idle_processing_time: total_idle_nanos
                .checked_div(idle_count)
                .map(Duration::from_nanos)
                .unwrap_or(Duration::ZERO)
                .min(max_idle_processing_time),
            max_idle_processing_time,
            uptime: self.uptime_baseline().elapsed(),
            last_activity: self.get_last_activity(),
        }
    }

    // Direct accessor methods for convenience

    /// Returns the total number of messages processed.
    #[inline]
    pub fn message_count(&self) -> u64 {
        self.message_count.load(Ordering::Relaxed)
    }

    /// Returns the average processing time per message.
    ///
    /// Clamped to [`max_processing_time`](Self::max_processing_time) so the
    /// `avg <= max` invariant holds even when this read races a recording
    /// (count/total/max are separate atomics).
    #[inline]
    pub fn avg_processing_time(&self) -> Duration {
        let count = self.message_count.load(Ordering::Relaxed);
        let total_nanos = self.total_processing_nanos.load(Ordering::Relaxed);
        total_nanos
            .checked_div(count)
            .map(Duration::from_nanos)
            .unwrap_or(Duration::ZERO)
            .min(self.max_processing_time())
    }

    /// Returns the maximum processing time observed.
    #[inline]
    pub fn max_processing_time(&self) -> Duration {
        Duration::from_nanos(self.max_processing_nanos.load(Ordering::Relaxed))
    }

    /// Returns the total number of priority messages processed.
    #[inline]
    pub fn priority_message_count(&self) -> u64 {
        self.priority_message_count.load(Ordering::Relaxed)
    }

    /// Returns the average priority message processing time.
    ///
    /// Clamped to [`max_priority_processing_time`](Self::max_priority_processing_time);
    /// see [`avg_processing_time`](Self::avg_processing_time).
    #[inline]
    pub fn avg_priority_processing_time(&self) -> Duration {
        let count = self.priority_message_count.load(Ordering::Relaxed);
        let total_nanos = self.total_priority_processing_nanos.load(Ordering::Relaxed);
        total_nanos
            .checked_div(count)
            .map(Duration::from_nanos)
            .unwrap_or(Duration::ZERO)
            .min(self.max_priority_processing_time())
    }

    /// Returns the maximum priority message processing time observed.
    #[inline]
    pub fn max_priority_processing_time(&self) -> Duration {
        Duration::from_nanos(self.max_priority_processing_nanos.load(Ordering::Relaxed))
    }

    /// Returns the total number of idle events dispatched to `on_idle`.
    #[inline]
    pub fn idle_event_count(&self) -> u64 {
        self.idle_event_count.load(Ordering::Relaxed)
    }

    /// Returns the average `on_idle` processing time.
    ///
    /// Clamped to [`max_idle_processing_time`](Self::max_idle_processing_time);
    /// see [`avg_processing_time`](Self::avg_processing_time).
    #[inline]
    pub fn avg_idle_processing_time(&self) -> Duration {
        let count = self.idle_event_count.load(Ordering::Relaxed);
        let total_nanos = self.total_idle_processing_nanos.load(Ordering::Relaxed);
        total_nanos
            .checked_div(count)
            .map(Duration::from_nanos)
            .unwrap_or(Duration::ZERO)
            .min(self.max_idle_processing_time())
    }

    /// Returns the maximum `on_idle` processing time observed.
    #[inline]
    pub fn max_idle_processing_time(&self) -> Duration {
        Duration::from_nanos(self.max_idle_processing_nanos.load(Ordering::Relaxed))
    }

    /// Returns the uptime since actor start.
    #[inline]
    pub fn uptime(&self) -> Duration {
        self.uptime_baseline().elapsed()
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

// NOTE: recording is *not* RAII-based. The runtime calls `record_message` /
// `record_priority_message` explicitly after the handler future returns
// normally (see `process_envelope!` in actor.rs). A drop-guard would also fire
// when the actor task is cancelled mid-handler (`JoinHandle::abort`, runtime
// shutdown) — a path that is neither a panic nor a success — and would record
// a partially-processed message against the documented "successfully
// processed" counters.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_initial_state() {
        let collector = MetricsCollector::new();
        let snapshot = collector.snapshot();

        assert_eq!(snapshot.message_count, 0);
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
    fn test_uptime_increases() {
        let collector = MetricsCollector::new();
        let uptime1 = collector.uptime();
        std::thread::sleep(Duration::from_millis(10));
        let uptime2 = collector.uptime();

        assert!(uptime2 > uptime1);
    }

    #[test]
    fn test_mark_started_resets_uptime_baseline() {
        let collector = MetricsCollector::new();
        std::thread::sleep(Duration::from_millis(20));
        let before = collector.uptime();
        collector.mark_started();
        let after = collector.uptime();

        // After marking on_start completion, uptime is measured from ~now, so it
        // is smaller than the pre-mark value that included the 20ms sleep.
        assert!(
            after < before,
            "mark_started should reset the uptime baseline (before={before:?}, after={after:?})"
        );
    }
}
