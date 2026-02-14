# Communicating with Actors

Once an actor is spawned and you have its `ActorRef`, you can communicate with it by sending messages. `rsActor` provides two primary modes of message passing: "tell" (fire-and-forget) and "ask" (request-response).

All message sending methods are asynchronous and return a `Future` that resolves when the operation completes (e.g., message enqueued for `tell`, or reply received for `ask`).
