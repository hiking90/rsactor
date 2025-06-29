# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.0] - 2025-06-28

### Added
- `#[message_handlers]` attribute macro with `#[handler]` method attributes for simplified message handling
- Comprehensive migration guide for moving from `impl_message_handler!` to the new macro approach
- Better documentation showcasing the recommended patterns

### Deprecated
- `impl_message_handler!` macro - Use `#[message_handlers]` with `#[handler]` attributes instead
- `__impl_message_handler_body!` internal helper macro
- Manual `Message<T>` trait implementations when using the new macro approach

### Changed
- Documentation now prioritizes the `#[message_handlers]` approach as the recommended method
- Updated examples to demonstrate the new macro patterns
- Added migration timeline: deprecated macros will be removed in version 1.0

### Migration Guide
To migrate from the deprecated approach:

**Before:**
```rust
impl Message<MyMessage> for MyActor {
    type Reply = String;
    async fn handle(&mut self, msg: MyMessage, actor_ref: &ActorRef<Self>) -> Self::Reply {
        "response".to_string()
    }
}

impl_message_handler!(MyActor, [MyMessage]);
```

**After:**
```rust
#[message_handlers]
impl MyActor {
    #[handler]
    async fn handle_my_message(&mut self, msg: MyMessage, actor_ref: &ActorRef<Self>) -> String {
        "response".to_string()
    }
}
```

### Breaking Changes
None in this release. The deprecated macros continue to work but will emit deprecation warnings.

### Note
The deprecated `impl_message_handler!` macro will be removed in version 1.0. Please migrate to the new `#[message_handlers]` approach.
