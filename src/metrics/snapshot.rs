// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, SystemTime};

/// Immutable snapshot of actor metrics at a point in time.
///
/// This struct captures the current state of an actor's performance metrics.
/// All fields represent cumulative values since the actor started.
///
/// # Consistency Note
///
/// When reading metrics concurrently with message processing, each underlying
/// counter is read atomically but the snapshot as a whole may reflect
/// different points in time. Derived fields (the averages) are computed from
/// two separately-read counters and are clamped to their corresponding maxima
/// so that `avg <= max` always holds; the residual skew is bounded by a single
/// message's duration. This is acceptable for monitoring purposes where exact
/// consistency is not required.
///
/// There is also no ordering guarantee between a reply and the metrics that
/// recorded it: immediately after an `ask().await` resolves, the counters may
/// not yet include that message (recording happens after the reply is sent).
/// Tests asserting exact counts right after an `ask` should retry briefly.
///
/// # Example
///
/// ```rust,ignore
/// let metrics = actor_ref.metrics();
///
/// println!("Messages processed: {}", metrics.message_count);
/// println!("Average processing time: {:?}", metrics.avg_processing_time);
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct MetricsSnapshot {
    /// Total number of messages successfully processed by this actor.
    pub message_count: u64,

    /// Average time spent processing each message.
    ///
    /// Returns `Duration::ZERO` if no messages have been processed yet.
    pub avg_processing_time: Duration,

    /// Maximum time spent processing any single message.
    ///
    /// This is useful for identifying slow message handlers or potential bottlenecks.
    pub max_processing_time: Duration,

    /// Total number of priority messages processed by this actor.
    ///
    /// Counts messages delivered through the priority channel (enabled via
    /// [`SpawnOptions::with_priority`](crate::SpawnOptions::with_priority)). Comparing
    /// this to `message_count` lets callers detect priority-channel abuse such as
    /// starvation of regular messages.
    pub priority_message_count: u64,

    /// Average time spent processing each priority message.
    pub avg_priority_processing_time: Duration,

    /// Maximum time spent processing any single priority message.
    pub max_priority_processing_time: Duration,

    /// Time elapsed since the actor was started.
    ///
    /// This is measured from when the actor's `on_start` completed successfully
    /// (the point the actor enters its message loop). Before `on_start`
    /// completes, the value falls back to the time since the metrics collector
    /// was created at [`spawn`](crate::spawn).
    ///
    /// Note: metrics outlive the actor (they remain readable through retained
    /// `ActorRef`/`ActorWeak` handles for post-mortem analysis), and `uptime`
    /// is wall-clock based — it keeps growing after the actor has stopped.
    /// Combine with `last_activity` or
    /// [`ActorRef::is_alive`](crate::ActorRef::is_alive) when deciding whether
    /// the actor is still running.
    pub uptime: Duration,

    /// Timestamp of the last message processing completion.
    ///
    /// Returns `None` if no messages have been processed yet.
    pub last_activity: Option<SystemTime>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_snapshot_default() {
        let snapshot = MetricsSnapshot::default();

        assert_eq!(snapshot.message_count, 0);
        assert_eq!(snapshot.avg_processing_time, Duration::ZERO);
        assert_eq!(snapshot.max_processing_time, Duration::ZERO);
        assert_eq!(snapshot.uptime, Duration::ZERO);
        assert!(snapshot.last_activity.is_none());
    }

    #[test]
    fn test_metrics_snapshot_clone() {
        let original = MetricsSnapshot {
            message_count: 42,
            avg_processing_time: Duration::from_millis(100),
            max_processing_time: Duration::from_millis(500),
            priority_message_count: 7,
            avg_priority_processing_time: Duration::from_micros(80),
            max_priority_processing_time: Duration::from_millis(2),
            uptime: Duration::from_secs(60),
            last_activity: Some(SystemTime::now()),
        };

        let cloned = original.clone();

        assert_eq!(cloned.message_count, 42);
        assert_eq!(cloned.avg_processing_time, Duration::from_millis(100));
        assert_eq!(cloned.max_processing_time, Duration::from_millis(500));
        assert_eq!(cloned.priority_message_count, 7);
        assert_eq!(
            cloned.avg_priority_processing_time,
            Duration::from_micros(80)
        );
        assert_eq!(
            cloned.max_priority_processing_time,
            Duration::from_millis(2)
        );
        assert_eq!(cloned.uptime, Duration::from_secs(60));
        assert!(cloned.last_activity.is_some());
    }

    #[test]
    fn test_metrics_snapshot_debug() {
        let snapshot = MetricsSnapshot::default();
        let debug_str = format!("{:?}", snapshot);

        assert!(debug_str.contains("MetricsSnapshot"));
        assert!(debug_str.contains("message_count"));
    }
}
