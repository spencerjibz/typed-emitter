mod tester {
    pub use async_std::test;
}
#[cfg(test)]
mod typed_async_emitter_async_std {

    use super::*;
    use typed_emitter::TypedEmitter;

    #[tester::test]
    async fn test_async_event_emitter_new() {
        let emitter: TypedEmitter<String, i32, ()> = TypedEmitter::new();
        assert!(emitter.listeners.lock().unwrap().is_empty());
    }

    #[tester::test]
    async fn test_async_event_emitter_on() {
        let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
        let event = "test_event".to_string();

        let id = emitter.on(event.clone(), |value| async move {
            format!("Received: {}", value)
        });

        assert!(!id.is_empty());
        assert_eq!(emitter.listeners.lock().unwrap().len(), 1);
        assert_eq!(emitter.listeners.lock().unwrap()[&event].len(), 1);
    }

    #[tester::test]
    async fn test_async_event_emitter_once() {
        let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
        let event = "test_event".to_string();

        let id = emitter.once(event.clone(), |value| async move {
            format!("Received once: {}", value)
        });

        assert!(!id.is_empty());
        assert_eq!(emitter.listeners.lock().unwrap().len(), 1);
        assert_eq!(emitter.listeners.lock().unwrap()[&event].len(), 1);
        assert_eq!(emitter.listeners.lock().unwrap()[&event][0].limit, Some(1));
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
                result.push(format!("Received: {}", value));
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

        assert_eq!(emitter.listeners.lock().unwrap()[&event].len(), 1);

        let removed_id = emitter.remove_listener(&id);
        assert_eq!(removed_id, Some(id));
        assert!(emitter.listeners.lock().unwrap()[&event].is_empty());

        let non_existent_id = "non_existent_id".to_string();
        let removed_id = emitter.remove_listener(&non_existent_id);
        assert_eq!(removed_id, None);
    }
}
