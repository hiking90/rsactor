// Copyright 2022 Jeff Kim <hiking90@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Actor Metrics System
//!
//! This module provides per-actor performance metrics collection with zero overhead
//! when the `metrics` feature is disabled.
//!
//! # Features
//!
//! - **Message count**: Total number of messages processed
//! - **Processing time**: Average and maximum message processing times
//! - **Error count**: Total number of errors during message handling
//! - **Uptime**: Time elapsed since actor started
//! - **Last activity**: Timestamp of most recent message processing
//!
//! # Design Principles
//!
//! - **Lock-free**: Uses `AtomicU64` for all counters to minimize contention
//! - **Zero-cost abstraction**: Completely compiled out when feature is disabled
//! - **Delegate monitoring**: Framework exposes metrics; monitoring/alerting is user's responsibility
//!
//! # Example
//!
//! ```rust,ignore
//! use rsactor::{spawn, Actor, ActorRef};
//!
//! let (actor_ref, _) = spawn::<MyActor>(args);
//!
//! // After processing some messages...
//! let metrics = actor_ref.metrics();
//! println!("Processed {} messages", metrics.message_count);
//! println!("Avg processing time: {:?}", metrics.avg_processing_time);
//! println!("Max processing time: {:?}", metrics.max_processing_time);
//! ```

pub(crate) mod collector;
mod snapshot;

pub(crate) use collector::MetricsCollector;
pub use snapshot::MetricsSnapshot;
