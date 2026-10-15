/*!
   A strongly-typed version of [`async-event-emitter`](https://crates.io/crates/async-event-emitter)
  # Key Features
   - Strong types  for the event, its parameters and return Values
   - Support for any type of eventType (Strings, Enums, or any type that implements Hash, Eq and Clone)
   - Supports  for all common async runtimes (Tokio, async-std and smol)
   - Lock-free and thread safe
## Getting Started
#### tokio
```rust
use typed_emitter::TypedEmitter;
#[tokio::main]
async fn main () {
// Create a typed emitter with String event names, i32 parameters and String return values
let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
}

```
#### Async-std
```rust
use typed_emitter::TypedEmitter;
#[async_std::main]
async fn main () {
// Create a typed emitter with String event names, i32 parameters and String return values
let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
}

```
#### smol
```rust
use typed_emitter::TypedEmitter;
use macro_rules_attribute::apply;

#[apply(smol_macros::main)]
async fn main () {
// Create a typed emitter with String event names, i32 parameters and String return values
let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
}

```
## Basic Usage
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
emitter.remove_listener(id);

}

```
## Create a Global EventEmitter
You'll likely want to have a single EventEmitter instance that can be shared across files;<br>

After all, one of the main points of using an EventEmitter is to avoid passing down a value through several nested functions/types and having a global subscription service.

 ``` rust
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
    // We need to maintain a lock through the mutex so we can avoid data races
    EVENT_EMITTER.on("Hello".to_string(), |_: i32|  async {println!("hello there!")});
    EVENT_EMITTER.emit("Hello".to_string(), 1).await;
}

async fn random_function() {
    // When the <"Hello"> event is emitted in main.rs then print <"Random stuff!">
    EVENT_EMITTER.on("Hello".to_string(), |_: i32| async { println!("Random stuff!")});
}

```
## Emit multiple Events with the  same emitter using an Enum
You'll likely want to have a single EventEmitter instance for multiple events;<br>

 ```

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

  License: MIT
*/

use futures::future::{join_all, BoxFuture, Future, FutureExt};
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

pub type AsyncCB<T, R> = dyn Fn(T) -> BoxFuture<'static, R> + Send + Sync + 'static;
#[derive(Clone)]
pub struct TypedListener<T, R> {
    pub callback: Arc<AsyncCB<T, R>>,
    pub is_global: bool,
    pub limit: Option<Arc<AtomicU64>>,
    pub id: Uuid,
}
impl<T, R> PartialEq for TypedListener<T, R> {
    fn eq(&self, other: &Self) -> bool {
        self.limit.as_ref().map(|d| d.load(Ordering::Acquire))
            == other.limit.as_ref().map(|d| d.load(Ordering::Acquire))
            && self.id == other.id
            && self.is_global == other.is_global
    }
}

#[derive(Hash, PartialEq, Eq)]
enum Event<K> {
    All,
    Unique(K),
}
type Listeners<T, R> = papaya::HashMap<Uuid, TypedListener<T, R>>;
type ListenerMap<K, T, R> = Arc<papaya::HashMap<K, Listeners<T, R>>>;
#[derive(Clone)]
pub struct TypedEmitter<Key, CallBackParameter, CallBackReturnType = ()> {
    listeners: ListenerMap<Event<Key>, CallBackParameter, CallBackReturnType>,
}

impl<K: Hash + Eq + Clone, P: Clone, R> Default for TypedEmitter<K, P, R> {
    fn default() -> Self {
        Self {
            listeners: Default::default(),
        }
    }
}

fn claim<T, R>(listener: &TypedListener<T, R>) -> bool {
    listener.limit.as_ref().map_or(true, |limit| {
        // `try_update` replaces `fetch_update` only on newer toolchains; keep this for older Rust versions
        #[allow(deprecated)]
        let claimed = limit.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1));
        claimed.is_ok()
    })
}

impl<K: Hash + Eq + Clone, P: Clone + 'static, R: Clone + 'static> TypedEmitter<K, P, R> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the number of events
    pub fn event_count(&self) -> usize {
        self.listeners.len()
    }
    /// Returns the numbers of listners per event
    pub fn listener_count_by_event(&self, event: &K) -> usize {
        self.listeners
            .pin()
            .get(&Event::Unique(event.clone()))
            .map_or(0, papaya::HashMap::len)
    }

    pub fn listeners_by_event(&self, event: &K) -> Vec<TypedListener<P, R>> {
        self.listeners
            .pin()
            .get(&Event::Unique(event.clone()))
            .map(|listeners| listeners.pin().values().cloned().collect())
            .unwrap_or_default()
    }

    /// Emits an event of the given parameters and executes each callback that is listening to that event asynchronously runs  each callback.
    ///
    /// # Example
    ///
    /// ```
    /// use typed_emitter::TypedEmitter;
    ///
    /// // Emits the <"Some event"> event and a value <"Hello programmer">
    /// #[tokio::main]
    /// async fn main() {
    ///     let emitter: TypedEmitter<&str, &str, ()> = TypedEmitter::new();
    ///     emitter.emit("event", "Hello programmer!").await;
    /// }
    /// ```
    ///
    ///  
    pub async fn emit(&self, event: K, value: P) {
        let futures = {
            let events = self.listeners.pin();
            let mut futures = Vec::new();
            if let Some(listeners) = events.get(&Event::Unique(event)) {
                let listeners = listeners.pin();
                let mut expired = Vec::new();
                for (id, listener) in listeners.iter() {
                    if claim(listener) {
                        futures.push((listener.callback)(value.clone()));
                    } else {
                        expired.push(*id);
                    }
                }
                for id in expired {
                    listeners.remove(&id);
                }
            }
            if let Some(global) = events.get(&Event::All) {
                if let Some(listener) = global.pin().values().next() {
                    futures.push((listener.callback)(value));
                }
            }
            futures
        };

        join_all(futures.into_iter().map(|future| future.map(drop))).await;
    }

    /// Removes an event listener with the given id
    ///
    /// # Example
    ///
    /// ```
    /// use typed_emitter::TypedEmitter;
    /// let mut event_emitter = TypedEmitter::new();
    /// let listener_id =
    ///     event_emitter.on("Some event", |value: ()| async { println!("Hello world!") });
    /// println!("{:?}", event_emitter.event_count());
    ///
    /// // Removes the listener that we just added
    /// event_emitter.remove_listener(listener_id);
    /// ```
    pub fn remove_listener(&self, id_to_delete: Uuid) -> Option<Uuid> {
        let removed = self
            .listeners
            .pin()
            .values()
            .any(|listeners| listeners.pin().remove(&id_to_delete).is_some());
        removed.then_some(id_to_delete)
    }
    /// Adds an event listener that will only execute the callback by the times limted. - Then the listener will be deleted.
    /// Returns the id of the newly added listener.
    ///
    /// # Example
    ///
    /// ```
    /// use typed_emitter::TypedEmitter;
    /// #[tokio::main]
    /// async fn main() {
    ///     let mut event_emitter = TypedEmitter::new();
    ///
    ///     event_emitter.on_limited("Some event", Some(2), |value: ()| async {
    ///         println!("Hello world!")
    ///     });
    ///     event_emitter.emit("Some event", ()).await;
    ///     // >> "Hello world!"
    ///     event_emitter.emit("Some event", ()).await;
    ///     // >> "Hello world!"
    ///     event_emitter.emit("Some event", ()).await;
    ///     // >> <Nothing happens here since listener was deleted>
    /// }
    /// ```
    pub fn on_limited<F, C>(&self, event: K, limit: Option<u64>, callback: C) -> Uuid
    where
        C: Fn(P) -> F + Send + Sync + 'static,
        F: Future<Output = R> + Send + Sync + 'static,
    {
        let id = Uuid::new_v4();
        let parsed_callback = move |value: P| callback(value).boxed();
        let limit = limit.map(|u| Arc::new(AtomicU64::new(u)));
        let listener = TypedListener {
            is_global: false,
            id,
            limit,
            callback: Arc::new(parsed_callback),
        };

        self.listeners
            .pin()
            .get_or_insert_with(Event::Unique(event), Listeners::default)
            .pin()
            .insert(id, listener);
        id
    }

    /// Adds an event listener called for whenever every event is called
    /// Returns the id of the newly added listener.
    ///
    /// # Example
    /// ```rust
    /// use typed_emitter::TypedEmitter;
    /// #[tokio::main]
    /// async fn main() {
    ///     let mut event_emitter = TypedEmitter::new();
    ///     // this will print Hello world two because of
    ///     event_emitter.on_all(|value: ()| async { println!("Hello world!") });
    ///     event_emitter.emit("Some event", ()).await;
    ///     // >> "Hello world!"
    ///
    ///     event_emitter.emit("next event", ()).await;
    ///     // >> <Nothing happens here since listener was deleted>
    /// }
    /// ```
    pub fn on_all<F, C>(&self, callback: C) -> Uuid
    where
        C: Fn(P) -> F + Send + Sync + 'static,
        F: Future<Output = R> + Send + Sync + 'static,
    {
        let exists = self.listeners.pin().contains_key(&Event::All);
        assert!(!exists, "only one global listener is allowed");
        let id = Uuid::new_v4();
        let parsed_callback = move |value: P| callback(value).boxed();

        let listener = TypedListener {
            id,
            is_global: true,
            limit: None,
            callback: Arc::new(parsed_callback),
        };

        self.listeners
            .pin()
            .get_or_insert_with(Event::All, Listeners::default)
            .pin()
            .insert(id, listener);
        id
    }

    /// Adds an event listener that will only execute the callback once - Then the listener will be deleted.
    /// Returns the id of the newly added listener.
    ///
    /// # Example
    ///
    /// ```
    /// use typed_emitter::TypedEmitter;
    ///   #[tokio::main]
    ///   async fn main () {
    /// let mut event_emitter = TypedEmitter::new();
    ///
    /// event_emitter.once("Some event", |value: ()| async {println!("Hello world!")});
    /// event_emitter.emit("Some event", ()).await; // First event is emitted and the listener's callback is called once
    /// // >> "Hello world!"
    ///
    /// event_emitter.emit("Some event", ()).await;
    /// // >> <Nothing happens here since listener was deleted>
    /// }
    /// ```
    pub fn once<F, C>(&self, event: K, callback: C) -> Uuid
    where
        C: Fn(P) -> F + Send + Sync + 'static,
        F: Future<Output = R> + Send + Sync + 'static,
    {
        self.on_limited(event, Some(1), callback)
    }

    /// Adds an event listener with a callback that will get called whenever the given event is emitted.
    /// Returns the id of the newly added listener.
    ///
    /// # Example
    ///
    /// ```
    /// use typed_emitter::TypedEmitter;
    /// let mut event_emitter = TypedEmitter::new();
    /// // MUST also match the type that is being emitted (here we just use a throwaway `()` type since we don't care about using the `value`)
    ///  event_emitter.on("Some event", |value: ()| async { println!("Hello world!")});
    /// ```
    pub fn on<F, C>(&self, event: K, callback: C) -> Uuid
    where
        C: Fn(P) -> F + Send + Sync + 'static,
        F: Future<Output = R> + Send + Sync + 'static,
    {
        self.on_limited(event, None, callback)
    }
}

impl<T, R> std::fmt::Debug for TypedListener<T, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedListener")
            .field("id", &self.id)
            .field("is_global", &self.is_global)
            .field("limit", &self.limit)
            .finish()
    }
}
impl<Key, CallBackParameter, CallBackReturnType> std::fmt::Debug
    for TypedEmitter<Key, CallBackParameter, CallBackReturnType>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedEmitter").finish_non_exhaustive()
    }
}
