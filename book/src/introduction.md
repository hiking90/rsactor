# Introduction

Welcome to the rsActor User Guide!

`rsActor` is a lightweight, Tokio-based actor framework in Rust focused on providing a simple and efficient actor model for local, in-process systems. It emphasizes clean message-passing semantics and straightforward actor lifecycle management while maintaining high performance for Rust applications.

This guide will walk you through the core concepts, features, and usage patterns of `rsActor` to help you build robust and concurrent applications.

**Note:** This project is actively evolving. While core APIs are stable, some features may be refined in future releases.

## Core Features

*   **Minimalist Actor System**: Focuses on core actor model primitives without unnecessary complexity.
*   **Message Passing**: Comprehensive communication patterns (`ask`, `tell`, timeout variants, blocking versions).
*   **Clean Lifecycle Management**: `on_start`, `on_run`, and `on_stop` hooks provide intuitive actor lifecycle control.
*   **Graceful Termination**: Actors can be stopped gracefully or killed immediately, with differentiated cleanup via the `killed` parameter.
*   **Rich Result Types**: `ActorResult` enum captures detailed completion states and error information.
*   **Macro-Assisted Development**: `#[message_handlers]` and `#[derive(Actor)]` reduce boilerplate significantly.
*   **Type Safety**: Compile-time type safety with `ActorRef<T>` prevents runtime type errors.
*   **Handler Traits**: `TellHandler` and `AskHandler` enable type-erased message sending for heterogeneous actor collections.
*   **Actor Control**: `ActorControl` trait provides type-erased lifecycle management.
*   **Weak References**: `ActorWeak<T>` prevents circular dependencies without memory leaks.
*   **Dead Letter Tracking**: Automatic logging of undelivered messages with structured tracing.
*   **Optional Metrics**: Per-actor performance metrics (message count, processing time, error count) via the `metrics` feature.
*   **Optional Tracing Instrumentation**: `#[tracing::instrument]` spans for observability via the `tracing` feature.
*   **Minimal Constraints**: Only `Send` trait required for actor structs, enabling flexible internal state management.
