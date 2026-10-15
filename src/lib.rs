#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(
    missing_debug_implementations,
    missing_docs,
    clippy::pedantic,
    clippy::nursery
)]

use futures::future::{join_all, BoxFuture, Future, FutureExt};
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

/// Type-erased listener callback: takes the emitted value and returns a future.
pub type AsyncCB<T, R> = dyn Fn(T) -> BoxFuture<'static, R> + Send + Sync + 'static;
/// A registered event listener.
#[derive(Clone)]
pub struct TypedListener<T, R> {
    /// The callback run when the listener's event is emitted.
    pub callback: Arc<AsyncCB<T, R>>,
    /// Whether this is the global listener registered with `on_all`.
    pub is_global: bool,
    /// Remaining number of calls, or `None` if the listener never expires.
    pub limit: Option<Arc<AtomicU64>>,
    /// Unique id returned when the listener was registered.
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
/// A typed, lock-free async event emitter.
///
/// Clones share the same listeners.
#[derive(Clone)]
pub struct TypedEmitter<Key, CallBackParameter, CallBackReturnType = ()> {
    listeners: ListenerMap<Event<Key>, CallBackParameter, CallBackReturnType>,
}

impl<K: Hash + Eq + Clone, P: Clone, R> Default for TypedEmitter<K, P, R> {
    fn default() -> Self {
        Self {
            listeners: Arc::default(),
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
    /// Creates an emitter with no listeners.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the number of events
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.listeners.len()
    }
    /// Returns the numbers of listners per event
    #[must_use]
    pub fn listener_count_by_event(&self, event: &K) -> usize {
        self.listeners
            .pin()
            .get(&Event::Unique(event.clone()))
            .map_or(0, papaya::HashMap::len)
    }

    /// Returns all listeners on the specified event
    #[must_use]
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
    // The future is `Send` whenever `K`, `P` and `R` are
    #[allow(clippy::future_not_send)]
    pub async fn emit(&self, event: K, value: P) {
        let futures = {
            let events = self.listeners.pin();
            let mut futures = Vec::new();
            if let Some(listeners) = events.get(&Event::Unique(event)) {
                let listeners = listeners.pin();
                let mut expired = Vec::new();
                for (id, listener) in &listeners {
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
    // Callers often remove a listener without checking whether it existed
    #[allow(clippy::must_use_candidate)]
    pub fn remove_listener(&self, id_to_delete: Uuid) -> Option<Uuid> {
        let events = self.listeners.pin();
        let removed = events.iter().any(|(event, listeners)| {
            matches!(event, Event::Unique(_)) && listeners.pin().remove(&id_to_delete).is_some()
        });
        let removed_global = events
            .remove_if(&Event::All, |_, global| {
                global.pin().contains_key(&id_to_delete)
            })
            .is_ok_and(|global| global.is_some());
        (removed || removed_global).then_some(id_to_delete)
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
    /// # Panics
    ///
    /// Panics if a global listener is already registered.
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
        let id = Uuid::new_v4();
        let parsed_callback = move |value: P| callback(value).boxed();

        let listener = TypedListener {
            id,
            is_global: true,
            limit: None,
            callback: Arc::new(parsed_callback),
        };

        let global = Listeners::default();
        global.pin().insert(id, listener);
        let installed = self.listeners.pin().try_insert(Event::All, global).is_ok();
        assert!(installed, "only one global listener is allowed");
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
            .finish_non_exhaustive()
    }
}
impl<Key, CallBackParameter, CallBackReturnType> std::fmt::Debug
    for TypedEmitter<Key, CallBackParameter, CallBackReturnType>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedEmitter").finish_non_exhaustive()
    }
}
