use std::collections::VecDeque;
use std::hash::Hash;
use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc};

use futures::future::{BoxFuture, Future, FutureExt};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use uuid::Uuid;

pub type AsyncCB<T, R> = dyn Fn(T) -> BoxFuture<'static, R> + Send + Sync + 'static;
#[derive(Clone)]
pub struct TypedListener<T, R> {
    pub callback: Arc<AsyncCB<T, R>>,
    pub limit: Option<u64>,
    pub id: String,
}

type ListenerMap<K, T, R> = Arc<Mutex<HashMap<K, Vec<TypedListener<T, R>>>>>;
#[derive(Default, Clone)]
pub struct TypedEmitter<Key, CallBackParameter, CallBackReturnType> {
    pub listeners: ListenerMap<Key, CallBackParameter, CallBackReturnType>,
}

impl<K: Eq + Hash + Clone, P: Clone + Send + Sync + 'static, R: Send + 'static>
    TypedEmitter<K, P, R>
{
    pub fn new() -> Self {
        Self {
            listeners: Arc::default(),
        }
    }

    pub async fn emit(&self, event: K, value: P) {
        let mut futures = FuturesUnordered::new();
        if let Some(listeners) = self.listeners.lock().unwrap().get_mut(&event) {
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

        while futures.next().await.is_some() {}
    }

    pub fn remove_listener(&self, id_to_delete: &str) -> Option<String> {
        for (_, event_listeners) in self.listeners.lock().unwrap().iter_mut() {
            if let Some(index) = event_listeners
                .iter()
                .position(|listener| listener.id == id_to_delete)
            {
                event_listeners.remove(index);
                return Some(id_to_delete.to_string());
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

        let mut lock = self.listeners.lock().unwrap();

        let entry = lock.entry(event).or_default();
        entry.push(listener);

        id
    }

    pub fn once<F, C>(&self, event: K, callback: C) -> String
    where
        C: Fn(P) -> F + Send + Sync + 'static,
        F: Future<Output = R> + Send + Sync + 'static,
    {
        self.on_limited(event, Some(1), callback)
    }

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
        f.debug_struct("TypedAsyncEventEmitter")
            .finish_non_exhaustive()
    }
}
