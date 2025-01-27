/*!
   A strongly-typed version of [`async-event-emitter`](https://crates.io/crates/async-event-emitter)
  # Key Features
   - Strong types  for the event, its parameters and return Values
   - Support for any type of eventType (Strings, Enums, or any type that implements Hash, Eq and Clone)
   - Supports  for all common async runtimes (Tokio, async-std and smol)
   - Reduced dependencies (only futures and uuid)
   - Thread Safe
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
emitter.remove_listener(&id);

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

use futures::future::{BoxFuture, Future, FutureExt};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use std::collections::VecDeque;
use std::hash::Hash;
use std::sync::{Arc, RwLock};

use uuid::Uuid;

pub type AsyncCB<T, R> = dyn Fn(T) -> BoxFuture<'static, R> + Send + Sync + 'static;
#[derive(Clone)]
pub struct TypedListener<T, R> {
    pub callback: Arc<AsyncCB<T, R>>,
    pub limit: Option<u64>,
    pub id: String,
}

impl<T, P> PartialEq for TypedListener<T, P> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.limit == other.limit
    }
}

use dashmap::DashMap;
type ListenerMap<K, T, R> = Arc<DashMap<K, Vec<TypedListener<T, R>>>>;
#[derive(Clone)]
pub struct TypedEmitter<Key, CallBackParameter, CallBackReturnType> {
    pub listeners: ListenerMap<Key, CallBackParameter, CallBackReturnType>,
    pub all_listener: Arc<RwLock<Option<TypedListener<CallBackParameter, CallBackReturnType>>>>,
}

impl<K: Eq + Hash + Clone, P: Clone + Send + Sync + 'static, R: Send + 'static + Clone> Default
    for TypedEmitter<K, P, R>
{
    fn default() -> Self {
        Self {
            listeners: Default::default(),
            all_listener: Default::default(),
        }
    }
}

impl<K: Eq + Hash + Clone, P: Clone + Send + Sync + 'static, R: Send + 'static + Clone>
    TypedEmitter<K, P, R>
{
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the number of events
    pub fn event_count(&self) -> usize {
        self.listeners.len()
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
        let mut futures = FuturesUnordered::new();
        if let Some(mut listeners) = self.listeners.get_mut(&event) {
            let mut listeners_to_remove: VecDeque<usize> = VecDeque::new();

            for (index, listener) in listeners.iter_mut().enumerate() {
                let callback = Arc::clone(&listener.callback);
                let value = value.clone();

                match listener.limit {
                    None => futures.push(callback(value)),
                    Some(limit) => {
                        if limit != 0 {
                            futures.push(callback(value));

                            listener.limit = Some(limit - 1);
                        } else {
                            listeners_to_remove.push_back(index);
                        }
                    }
                }
            }
            while let Some(index) = listeners_to_remove.pop_front() {
                listeners.remove(index);
            }
        }
        // fire to the global listener;
        if let Some(global_listener) = self.all_listener.read().unwrap().as_ref() {
            let callback = Arc::clone(&global_listener.callback);
            let value = value.clone();
            futures.push(callback(value))
        }

        while futures.next().await.is_some() {}
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
    /// println!("{:?}", event_emitter.listeners);
    ///
    /// // Removes the listener that we just added
    /// event_emitter.remove_listener(&listener_id);
    /// ```
    pub fn remove_listener(&self, id_to_delete: &str) -> Option<String> {
        for mut mult_ref in self.listeners.iter_mut() {
            let event_listeners = mult_ref.value_mut();
            if let Some(index) = event_listeners
                .iter()
                .position(|listener| listener.id == id_to_delete)
            {
                event_listeners.remove(index);
                return Some(id_to_delete.to_string());
            }
        }
        let all_listener = self.all_listener.read().unwrap().clone();
        // check if the id matches that of the global listener;
        if let Some(all_listener) = all_listener.as_ref() {
            if id_to_delete == all_listener.id {
                self.all_listener.write().unwrap().take();
            }
        }

        None
    }

    fn on_limited<F, C>(&self, event: K, limit: Option<u64>, callback: C) -> String
    where
        C: Fn(P) -> F + Send + Sync + 'static,
        F: Future<Output = R> + Send + Sync + 'static,
    {
        let id = Uuid::new_v4().to_string();
        let parsed_callback = move |value: P| callback(value).boxed();

        let listener = TypedListener {
            id: id.clone(),
            limit,
            callback: Arc::new(parsed_callback),
        };

        let mut entry = self.listeners.entry(event).or_default();
        entry.push(listener);

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
    pub fn on_all<F, C>(&self, callback: C) -> String
    where
        C: Fn(P) -> F + Send + Sync + 'static,
        F: Future<Output = R> + Send + Sync + 'static,
    {
        assert!(
            self.all_listener.read().unwrap().is_none(),
            "only one global listener is allowed"
        );
        let id = Uuid::new_v4().to_string();
        let parsed_callback = move |value: P| callback(value).boxed();

        let listener = TypedListener {
            id: id.clone(),
            limit: None,
            callback: Arc::new(parsed_callback),
        };

        self.all_listener.write().unwrap().replace(listener.clone());

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
    pub fn once<F, C>(&self, event: K, callback: C) -> String
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
    pub fn on<F, C>(&self, event: K, callback: C) -> String
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
