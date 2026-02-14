# Summary

[Introduction](introduction.md)

# User Guide
- [Getting Started](getting_started.md)
- [Core Concepts](core_concepts.md)
    - [What is an Actor?](core_concepts/what_is_an_actor.md)
    - [Actor References (`ActorRef`)](core_concepts/actor_ref.md)
    - [Messages](core_concepts/messages.md)
    - [Actor Lifecycle](core_concepts/actor_lifecycle.md)
- [Creating Actors](creating_actors.md)
    - [Basic Actors](creating_actors/basic_actors.md)
    - [Async Actors](creating_actors/async_actors.md)
    - [Blocking Task Actors](creating_actors/blocking_task_actors.md)
- [Communicating with Actors](communicating_with_actors.md)
    - [Tell (Fire and Forget)](communicating_with_actors/tell.md)
    - [Ask (Request-Response)](communicating_with_actors/ask.md)
- [Error Handling](error_handling.md)
    - [`ActorResult`](error_handling/actor_result.md)
- [Timeouts](timeouts.md)
- [Macros](macros.md)
    - [Message Handlers Macro](macros/unified_macro.md)

# Advanced Features
- [Advanced Features](advanced.md)
    - [Handler Traits](advanced/handler_traits.md)
    - [Actor Control](advanced/actor_control.md)
    - [Weak References](advanced/weak_references.md)
    - [Metrics](advanced/metrics.md)
    - [Dead Letter Tracking](advanced/dead_letters.md)
    - [Tracing & Observability](advanced/tracing.md)

# Examples
- [Examples](examples.md)
    - [Basic Usage](examples/basic.md)
    - [Async Worker](examples/async_worker.md)
    - [Blocking Task](examples/blocking_task.md)
    - [Actor with Timeout](examples/actor_with_timeout.md)
    - [Kill Demo](examples/kill_demo.md)
    - [Ask Join](examples/ask_join.md)
    - [Handler Demo](examples/handler_demo.md)
    - [Weak References](examples/weak_references.md)
    - [Metrics](examples/metrics.md)
    - [Tracing](examples/tracing.md)
    - [Dining Philosophers](examples/dining_philosophers.md)

# Reference
- [FAQ](faq.md)
