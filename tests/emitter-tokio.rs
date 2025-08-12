#![allow(dead_code, unused)]
mod tester {
    pub use tokio::test;
}
#[cfg(test)]
mod typed_async_emitter_tokio {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;
    use typed_emitter::TypedEmitter;
    use uuid::Uuid;

    #[tester::test]
    async fn test_async_event_emitter_new() {
        let emitter: TypedEmitter<String, i32, ()> = TypedEmitter::new();
        assert!(emitter.listeners.is_empty());
    }

    #[tester::test]
    async fn test_async_event_emitter_on() {
        let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
        let event = "test_event".to_string();

        let id = emitter.on(event.clone(), |value| async move {
            format!("Received: {value}")
        });

        assert_eq!(emitter.listeners.get(&event).unwrap().len(), 1);
        assert_eq!(emitter.listeners.get(&event).unwrap().len(), 1);
    }

    #[tester::test]
    async fn test_async_event_emitter_once() {
        let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
        let event = "test_event".to_string();

        let id = emitter.once(event.clone(), |value| async move {
            format!("Received once: {value}")
        });

        assert_eq!(emitter.listeners.get(&event).unwrap().len(), 1);
        assert_eq!(emitter.listeners.get(&event).unwrap().len(), 1);
        assert_eq!(emitter.listeners.get(&event).unwrap()[0].limit, Some(1));
    }

    #[tester::test]
    async fn test_async_event_emitter_emit() {
        use futures::lock::Mutex;
        use std::sync::Arc;

        let emitter = TypedEmitter::new();
        let event = "test_event".to_string();
        let result = Arc::new(Mutex::new(Vec::new()));

        let result_clone = Arc::clone(&result);
        emitter.on(event.clone(), move |value| {
            let result = Arc::clone(&result_clone);
            async move {
                let mut result = result.lock().await;
                result.push(format!("Received: {value}"));
                "OK".to_string()
            }
        });

        emitter.emit(event, 42).await;

        let result = result.lock().await;
        assert_eq!(result.len(), 1);

        assert_eq!(result[0], "Received: 42");
    }

    #[tester::test]
    async fn test_async_event_emitter_remove_listener() {
        let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
        let event = "test_event".to_string();

        let id = emitter.on(event.clone(), |_| async { "OK".to_string() });

        assert_eq!(emitter.listeners.get(&event).unwrap().len(), 1);

        let removed_id = emitter.remove_listener(id);
        assert_eq!(removed_id, Some(id));
        assert!(emitter.listeners.get(&event).unwrap().is_empty());

        let non_existent_id = Uuid::new_v4();
        let removed_id = emitter.remove_listener(non_existent_id);
        assert_eq!(removed_id, None);
    }

    #[tester::test]
    async fn global_listener_on_emitter_works() {
        let instance = TypedEmitter::new();
        let emit_count = Arc::new(AtomicUsize::new(0));
        let count_clone = emit_count.clone();
        let callback = |value: Arc<AtomicUsize>| async move {
            value.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            value.clone()
        };

        let id = instance.on_all(callback);
        instance.on("test_event", callback);
        instance.emit("test_event", count_clone).await;
        assert!(instance.all_listener.read().unwrap().is_some());
        assert_eq!(emit_count.load(std::sync::atomic::Ordering::SeqCst), 2);
        instance.remove_listener(id);
        assert!(instance.all_listener.read().unwrap().is_none());
    }
}
