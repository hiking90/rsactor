// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Stress test for metrics snapshot consistency.
//!
//! The collector's counters are separate Relaxed atomics, so a reader racing
//! a recording can observe a torn (count, total, max) combination — most
//! visibly on an actor's *first* messages, where count/total are already
//! updated but the max is not yet, yielding `avg > max == 0`. Before the
//! clamp in `snapshot()` and the avg accessors this reproduced within a few
//! thousand rounds of the loop below. The test asserts the documented
//! `avg <= max` invariant never breaks.

use rsactor::{message_handlers, spawn, Actor, ActorRef};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Actor, Debug)]
struct Worker;

#[derive(Debug)]
struct Work;

#[message_handlers]
impl Worker {
    #[handler]
    async fn handle_work(&mut self, _: Work, _: &ActorRef<Self>) {
        // A short spin keeps recorded durations non-zero.
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_micros(2) {
            std::hint::spin_loop();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_avg_never_exceeds_max() {
    const ROUNDS: usize = 4_000;
    const MESSAGES_PER_ROUND: u64 = 4;

    let mut violations = 0u64;
    let mut reads = 0u64;

    // Fresh actor per round: tearing is only observable while the counters
    // are small (a single torn message shifts the derived avg by a large
    // fraction), so a long-lived actor would mask the race.
    for _ in 0..ROUNDS {
        let (actor_ref, handle) = spawn::<Worker>(Worker);

        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = stop.clone();
        let reader_ref = actor_ref.clone();
        let reader = tokio::spawn(async move {
            let mut v = 0u64;
            let mut r = 0u64;
            while !reader_stop.load(Ordering::Relaxed) {
                let s = reader_ref.metrics();
                if s.message_count > 0 && s.avg_processing_time > s.max_processing_time {
                    v += 1;
                }
                // The direct accessors read the atomics independently of
                // snapshot(); exercise their clamp too.
                if reader_ref.message_count() > 0
                    && reader_ref.avg_processing_time() > reader_ref.max_processing_time()
                {
                    v += 1;
                }
                r += 2;
                tokio::task::yield_now().await;
            }
            (v, r)
        });

        for _ in 0..MESSAGES_PER_ROUND {
            actor_ref.tell(Work).await.unwrap();
        }
        while actor_ref.message_count() < MESSAGES_PER_ROUND {
            tokio::task::yield_now().await;
        }

        stop.store(true, Ordering::Relaxed);
        let (v, r) = reader.await.unwrap();
        violations += v;
        reads += r;

        actor_ref.stop().await;
        let _ = handle.await;
    }

    assert!(reads > 0, "reader must have raced the writer");
    assert_eq!(
        violations, 0,
        "avg_processing_time must never exceed max_processing_time \
         (clamped in snapshot() and the accessors); {reads} racing reads"
    );
}
