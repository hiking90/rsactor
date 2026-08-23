- When called from inside a [`LocalSet`](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html)
  running on a multi-thread runtime: the runtime handle reports multi-thread
  flavor, but `block_in_place` is not permitted there.
- When called from an async context that `block_in_place` cannot rescue — a
  task on a `current_thread` runtime, or any thread driving a `current_thread`
  runtime's `Runtime::block_on`/`Handle::block_on` (a *multi-thread* runtime's
  `block_on` driver takes the `block_in_place` path and is fine) — this panics
  with Tokio's "Cannot block the current thread from within a runtime".
  Parking such a thread would freeze its runtime, starving every actor on it:
  for the full timeout where one is in play, and forever where there is none —
  a silent hang that not even `kill()` could break. The loud panic surfaces the
  bug instead. Call from a [`spawn_blocking`](tokio::task::spawn_blocking)
  thread, or use this method's async counterpart, instead.
- When a timeout is in play and the caller's own multi-thread runtime was built
  **without a time driver** (no `enable_time()` / `enable_all()`): the
  `block_in_place` path awaits the timeout on that runtime, so
  `tokio::time::timeout` panics because no timer is available. The slow-path
  temporary runtime always enables timers, so only the caller's own runtime is
  affected. On the two methods whose `timeout` is optional
  (`blocking_tell`/`blocking_ask`), passing `None` avoids this condition
  entirely; the `*_priority` methods always take a deadline.
