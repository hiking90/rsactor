// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Message-path microbenchmarks.
//!
//! Goal: quantify how much of the per-message cost is attributable to the
//! internal `ActorRef::clone()` embedded in every `tell`/`ask` envelope,
//! relative to the unavoidable costs (heap allocation of the payload + boxed
//! future, channel push, oneshot round-trip, task wakeup).
//!
//! Compare the `actorref_clone` number against `tell_single`: that ratio is
//! the fraction of a send that the clone (atomic refcount traffic) accounts
//! for. If it is small, the `Arc<Inner>` refactor is not worth it.
//!
//! Run:
//!   cargo bench --bench message_throughput
//!   cargo bench --bench message_throughput --features metrics   # extra Arc in clone

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use rsactor::{message_handlers, spawn_with_options, Actor, ActorRef, SpawnOptions};
use tokio::runtime::Runtime;

#[derive(Actor)]
struct BenchActor;

/// Smallest possible message — isolates framework overhead from payload cost.
struct Noop;

/// A message carrying a small payload, to see the marginal cost of a non-ZST.
struct Payload(#[allow(dead_code)] u64);

#[message_handlers]
impl BenchActor {
    #[handler]
    async fn handle_noop(&mut self, _msg: Noop, _: &ActorRef<Self>) -> () {}

    #[handler]
    async fn handle_payload(&mut self, _msg: Payload, _: &ActorRef<Self>) -> u64 {
        0
    }
}

fn multi_thread_rt() -> Runtime {
    // Multi-thread so the actor task runs on a different worker than the
    // sender: this exposes the cross-thread atomic decrement when the embedded
    // ActorRef is dropped on the actor side (the realistic cache-line case).
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap()
}

fn bench_clone(c: &mut Criterion) {
    let rt = multi_thread_rt();
    // Spawn a live actor so the senders are real and open.
    let actor_ref = rt.block_on(async {
        let (aref, _join) = spawn_with_options::<BenchActor>(
            BenchActor,
            SpawnOptions::new().mailbox_capacity(1024),
        );
        aref
    });

    let mut group = c.benchmark_group("clone");
    // Raw, uncontended clone+drop cost — the atomic refcount traffic only.
    group.bench_function("actorref_clone", |b| {
        b.iter(|| {
            let c = black_box(actor_ref.clone());
            black_box(&c);
            // c dropped here -> decrements on the same thread (uncontended)
        });
    });
    group.finish();

    drop(actor_ref);
    rt.block_on(async { tokio::task::yield_now().await });
}

fn bench_tell(c: &mut Criterion) {
    let rt = multi_thread_rt();
    let actor_ref = rt.block_on(async {
        let (aref, _join) = spawn_with_options::<BenchActor>(
            BenchActor,
            // Large mailbox so the batch send never hits backpressure: we want
            // pure send cost, not the actor's drain rate.
            SpawnOptions::new().mailbox_capacity(200_000),
        );
        aref
    });

    // Identical actor but with the idle channel ENABLED, for a same-binary A/B
    // that isolates the cost of the always-active idle_subscribe select! branch.
    let actor_ref_idle = rt.block_on(async {
        let (aref, _join) = spawn_with_options::<BenchActor>(
            BenchActor,
            SpawnOptions::new().mailbox_capacity(200_000).with_idle(),
        );
        aref
    });

    let mut group = c.benchmark_group("tell");

    // Single fire-and-forget send: clone + Box payload + boxed future + channel push.
    // idle OFF (default after opt-in change).
    group.bench_function("tell_single", |b| {
        b.to_async(&rt).iter(|| {
            let aref = &actor_ref;
            async move {
                aref.tell(Noop).await.unwrap();
            }
        });
    });
    // idle ON: same path + one extra always-active select! branch in the actor loop.
    group.bench_function("tell_single_idle_on", |b| {
        b.to_async(&rt).iter(|| {
            let aref = &actor_ref_idle;
            async move {
                aref.tell(Noop).await.unwrap();
            }
        });
    });

    // Throughput: send N, then a final `ask` to guarantee the whole batch was
    // drained before the iteration ends (keeps the mailbox from accumulating
    // across iterations and gives a real messages/sec number).
    const N: u64 = 1000;
    group.throughput(Throughput::Elements(N));
    group.bench_function("tell_batch_1000", |b| {
        b.to_async(&rt).iter(|| {
            let aref = &actor_ref;
            async move {
                for _ in 0..N {
                    aref.tell(Noop).await.unwrap();
                }
                // Sync point: this reply only arrives after all N noops processed.
                let _ = black_box(aref.ask(Payload(0)).await.unwrap());
            }
        });
    });

    // idle ON throughput counterpart.
    group.throughput(Throughput::Elements(N));
    group.bench_function("tell_batch_1000_idle_on", |b| {
        b.to_async(&rt).iter(|| {
            let aref = &actor_ref_idle;
            async move {
                for _ in 0..N {
                    aref.tell(Noop).await.unwrap();
                }
                let _ = black_box(aref.ask(Payload(0)).await.unwrap());
            }
        });
    });

    group.finish();
    drop(actor_ref);
    drop(actor_ref_idle);
}

fn bench_ask(c: &mut Criterion) {
    let rt = multi_thread_rt();
    let actor_ref = rt.block_on(async {
        let (aref, _join) = spawn_with_options::<BenchActor>(
            BenchActor,
            SpawnOptions::new().mailbox_capacity(1024),
        );
        aref
    });

    let mut group = c.benchmark_group("ask");
    // Full round-trip: send + oneshot + handler + reply box + downcast + 2 wakeups.
    group.bench_function("ask_roundtrip", |b| {
        b.to_async(&rt).iter(|| {
            let aref = &actor_ref;
            async move {
                let _ = black_box(aref.ask(Payload(0)).await.unwrap());
            }
        });
    });
    group.finish();
    drop(actor_ref);
}

fn bench_fan_in(c: &mut Criterion) {
    // Worst case for the embedded-clone cost: many sender tasks concurrently
    // tell one actor. Every envelope's actor_ref is dropped on the SINGLE
    // actor thread, so all the decrement traffic (3 atomics today, 1 with
    // Arc<Inner>) concentrates on the one bottleneck thread. If clone cost
    // mattered anywhere, it would show up here.
    let rt = multi_thread_rt();
    let actor_ref = rt.block_on(async {
        let (aref, _join) = spawn_with_options::<BenchActor>(
            BenchActor,
            SpawnOptions::new().mailbox_capacity(200_000),
        );
        aref
    });

    const SENDERS: usize = 8;
    const PER_SENDER: u64 = 1000;
    const TOTAL: u64 = SENDERS as u64 * PER_SENDER;

    // Proxy for the Arc<Inner> delta: a priority-enabled actor embeds 4 senders
    // per envelope instead of 3, so this measures the marginal throughput cost
    // of ONE extra cloned sender. If 3->4 barely moves the number, then 3->1
    // (what Arc<Inner> achieves) barely helps either.
    let actor_ref_prio = rt.block_on(async {
        let (aref, _join) = spawn_with_options::<BenchActor>(
            BenchActor,
            SpawnOptions::new()
                .mailbox_capacity(200_000)
                .with_priority(),
        );
        aref
    });

    let run = |aref: ActorRef<BenchActor>| async move {
        let mut handles = Vec::with_capacity(SENDERS);
        for _ in 0..SENDERS {
            let a = aref.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..PER_SENDER {
                    a.tell(Noop).await.unwrap();
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        // Drain barrier: ensure the actor processed (and dropped) every
        // embedded actor_ref before the iteration ends.
        let _ = black_box(aref.ask(Payload(0)).await.unwrap());
    };

    let mut group = c.benchmark_group("fan_in");
    group.throughput(Throughput::Elements(TOTAL));
    // 3 senders embedded per envelope (mailbox, terminate, idle_subscribe).
    group.bench_function("tell_8x1000_3senders", |b| {
        b.to_async(&rt).iter(|| run(actor_ref.clone()));
    });
    // 4 senders embedded per envelope (+ priority). Same workload.
    group.bench_function("tell_8x1000_4senders", |b| {
        b.to_async(&rt).iter(|| run(actor_ref_prio.clone()));
    });
    group.finish();
    drop(actor_ref);
    drop(actor_ref_prio);
}

fn bench_select_branches(c: &mut Criterion) {
    // Isolates the cost the actor loop pays per message for each ALWAYS-ACTIVE
    // `select!` recv branch (terminate, idle_subscribe, [priority], mailbox).
    // Each variant drains N messages from one "hot" channel while K-1 extra
    // channels sit empty-but-open (held senders) — exactly what the runtime
    // does. The slope across b1..b4 is the marginal per-branch cost. The real
    // loop is b3 (no priority) / b4 (priority).
    use tokio::sync::mpsc;
    let rt = multi_thread_rt();
    const N: u64 = 2000;

    let mut group = c.benchmark_group("select_branches");
    group.throughput(Throughput::Elements(N));

    // 1 branch: hot only.
    group.bench_function("b1", |b| {
        b.to_async(&rt).iter_batched(
            || {
                let (tx, rx) = mpsc::channel::<u32>(N as usize);
                for i in 0..N as u32 {
                    tx.try_send(i).unwrap();
                }
                (rx, tx)
            },
            |(mut rx, _tx)| async move {
                let mut sum = 0u64;
                for _ in 0..N {
                    tokio::select! { biased; Some(v) = rx.recv() => sum += v as u64 }
                }
                black_box(sum)
            },
            BatchSize::LargeInput,
        );
    });

    // 2 branches: 1 empty + hot.
    group.bench_function("b2", |b| {
        b.to_async(&rt).iter_batched(
            || {
                let (tx, rx) = mpsc::channel::<u32>(N as usize);
                for i in 0..N as u32 {
                    tx.try_send(i).unwrap();
                }
                let e1 = mpsc::channel::<u32>(1);
                (rx, tx, e1)
            },
            |(mut rx, _tx, (e1t, mut e1r))| async move {
                let _keep = e1t;
                let mut sum = 0u64;
                for _ in 0..N {
                    tokio::select! { biased;
                        Some(v) = e1r.recv() => sum += v as u64,
                        Some(v) = rx.recv() => sum += v as u64,
                    }
                }
                black_box(sum)
            },
            BatchSize::LargeInput,
        );
    });

    // 3 branches: 2 empty + hot  (== real loop WITHOUT priority).
    group.bench_function("b3_default_loop", |b| {
        b.to_async(&rt).iter_batched(
            || {
                let (tx, rx) = mpsc::channel::<u32>(N as usize);
                for i in 0..N as u32 {
                    tx.try_send(i).unwrap();
                }
                (rx, tx, mpsc::channel::<u32>(1), mpsc::channel::<u32>(1))
            },
            |(mut rx, _tx, (e1t, mut e1r), (e2t, mut e2r))| async move {
                let _k = (e1t, e2t);
                let mut sum = 0u64;
                for _ in 0..N {
                    tokio::select! { biased;
                        Some(v) = e1r.recv() => sum += v as u64,
                        Some(v) = e2r.recv() => sum += v as u64,
                        Some(v) = rx.recv() => sum += v as u64,
                    }
                }
                black_box(sum)
            },
            BatchSize::LargeInput,
        );
    });

    // 4 branches: 3 empty + hot  (== real loop WITH priority).
    group.bench_function("b4_priority_loop", |b| {
        b.to_async(&rt).iter_batched(
            || {
                let (tx, rx) = mpsc::channel::<u32>(N as usize);
                for i in 0..N as u32 {
                    tx.try_send(i).unwrap();
                }
                (
                    rx,
                    tx,
                    mpsc::channel::<u32>(1),
                    mpsc::channel::<u32>(1),
                    mpsc::channel::<u32>(1),
                )
            },
            |(mut rx, _tx, (e1t, mut e1r), (e2t, mut e2r), (e3t, mut e3r))| async move {
                let _k = (e1t, e2t, e3t);
                let mut sum = 0u64;
                for _ in 0..N {
                    tokio::select! { biased;
                        Some(v) = e1r.recv() => sum += v as u64,
                        Some(v) = e2r.recv() => sum += v as u64,
                        Some(v) = e3r.recv() => sum += v as u64,
                        Some(v) = rx.recv() => sum += v as u64,
                    }
                }
                black_box(sum)
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_box_baseline(c: &mut Criterion) {
    // Reference point: a single heap allocation of the same shape the framework
    // does per message (Box<dyn ...>). Lets us compare clone vs alloc directly.
    let mut group = c.benchmark_group("baseline");
    group.bench_function("box_alloc", |b| {
        b.iter_batched(
            || (),
            |_| {
                let b: Box<dyn Send> = Box::new(black_box(Noop));
                black_box(b)
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_clone,
    bench_box_baseline,
    bench_select_branches,
    bench_tell,
    bench_fan_in,
    bench_ask
);
criterion_main!(benches);
