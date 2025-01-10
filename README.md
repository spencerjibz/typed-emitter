### typed-emitter

[![Crates.io](https://img.shields.io/crates/v/typed-emitter)](https://crates.io/crates/typed-emitter)
[![docs.rs](https://img.shields.io/docsrs/async-event-emitter)](https://docs.rs/typed-emitter/)
[![CI](https://github.com/spencerjibz/typed-emitter/actions/workflows/ci.yml/badge.svg)](https://github.com/spencerjibz/typed-emitter/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A strongly-typed version of [`async-event-emitter`](https://crates.io/crates/async-event-emitter)

#### Key Features

- Strong types  for event-type, its parameters and return Values
- Support for any type of eventType (Strings, Enums, or any type that implements Hash, Eq and Clone)
- Supports for all common async runtimes (Tokio, async-std and smol)
- Reduced dependencies (only futures and uuid)
- Thread Safe

#### Getting Started

##### tokio

```rust
use typed_emitter::TypedEmitter;
#[tokio::main]
async fn main () {
// Create a typed emitter with String event names, i32 parameters and String return values
let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
}

```

##### Async-std

```rust
use typed_emitter::TypedEmitter;
#[async_std::main]
async fn main () {
// Create a typed emitter with String event names, i32 parameters and String return values
let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
}

```

##### smol

```rust
use typed_emitter::TypedEmitter;
use macro_rules_attribute::apply;

#[apply(smol_macros::main)]
async fn main () {
// Create a typed emitter with String event names, i32 parameters and String return values
let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
}

```

#### Basic Usage

```rust
use typed_emitter::TypedEmitter;
#[tokio::main]
async fn main () {
// Create a typed emitter with String event names, i32 parameters and String return values
let emitter = TypedEmitter::new();
// Add a persistent listener
let id = emitter.on("event".to_string(), |value| async move {
    format!("Received: {}", value)
});

// Add a one-time listener
emitter.once("event".to_string(), |value| async move {
    format!("Received once: {}", value)
});

// Emit an event
emitter.emit("event".to_string(), 42).await;

// Remove a listener by ID
emitter.remove_listener(&id);

}

```

#### Create a Global EventEmitter

You'll likely want to have a single EventEmitter instance that can be shared across files;<br>

After all, one of the main points of using an EventEmitter is to avoid passing down a value through several nested functions/types and having a global subscription service.

```rust
// global_event_emitter.rs
use lazy_static::lazy_static;

use typed_emitter::TypedEmitter;

// Use lazy_static! because the size of EventEmitter is not known at compile time
lazy_static! {
   // Export the emitter with `pub` keyword
   pub static ref EVENT_EMITTER: TypedEmitter<String, i32, ()> = TypedEmitter::new();
}

#[tokio::main]
async fn main() {
   EVENT_EMITTER.on("Hello".to_string(), |_: i32|  async {println!("hello there!")});
   EVENT_EMITTER.emit("Hello".to_string(), 1).await;
}

async fn random_function() {
   // When the <"Hello"> event is emitted in main.rs then print <"Random stuff!">
   EVENT_EMITTER.on("Hello".to_string(), |_: i32| async { println!("Random stuff!")});
}

```

#### Emit multiple Events with the same emitter using an Enum

You'll likely want to have a single EventEmitter instance for multiple events;<br>

```rust

use typed_emitter::TypedEmitter;
#[derive(Eq,PartialEq, Clone, Hash)]
enum JobState {
     Closed,
     Completed,
     Failed ,
    Stalled
 }

#[tokio::main]
async fn main() {
let emitter = TypedEmitter::new();
 emitter.on_all(|done: Option<&str>| async move {
        println!("{done:?}");
  }); // prints None, failed, Stalled, completed
 emitter.emit(JobState::Closed, None).await;
 emitter.emit(JobState::Failed, Some("failed")).await;
 emitter.emit(JobState::Stalled, Some("Stalled")).await;
 emitter.emit(JobState::Completed, Some("Completed")).await;
}

```

#### License

MIT License (MIT), see [LICENSE](LICENSE)
