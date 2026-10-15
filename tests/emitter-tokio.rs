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
        assert_eq!(emitter.event_count(), 0);
    }

    #[tester::test]
    async fn test_async_event_emitter_on() {
        let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
        let event = "test_event".to_string();

        let id = emitter.on(event.clone(), |value| async move {
            format!("Received: {value}")
        });

        assert_eq!(emitter.listener_count_by_event(&event), 1);
    }

    #[tester::test]
    async fn test_async_event_emitter_once() {
        let emitter: TypedEmitter<String, i32, String> = TypedEmitter::new();
        let event = "test_event".to_string();

        let id = emitter.once(event.clone(), |value| async move {
            format!("Received once: {value}")
        });

        assert_eq!(emitter.listener_count_by_event(&event), 1);
        assert_eq!(
            emitter.listeners_by_event(&event)[0]
                .limit
                .clone()
                .unwrap_or_default()
                .load(std::sync::atomic::Ordering::Acquire),
            1
        );
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

        assert_eq!(emitter.listener_count_by_event(&event), 1);

        let removed_id = emitter.remove_listener(id);
        assert_eq!(removed_id, Some(id));
        assert!(emitter.listeners_by_event(&event).is_empty());

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
        assert_eq!(emit_count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
    #[tester::test]
    async fn test_limited_listener() {
        let instance = TypedEmitter::new();
        let emit_count = Arc::new(AtomicUsize::new(0));
        let count_clone = emit_count.clone();
        let callback = |value: Arc<AtomicUsize>| async move {
            value.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            dbg!("hello");
            value.clone()
        };
        let event = "test_event";

        instance.on_limited(event, Some(3), callback);
        instance.emit(event, count_clone.clone()).await;
        instance.emit(event, count_clone.clone()).await;
        instance.emit(event, count_clone.clone()).await;
        let mut listeners = instance.listeners_by_event(&event);
        // we expect the listener limit to be zero by now;
        if let Some(listener) = listeners.pop() {
            let limit = listener
                .limit
                .map(|d| d.load(std::sync::atomic::Ordering::Acquire));
            assert_eq!(limit, Some(0))
        }
        instance.emit(event, count_clone.clone()).await;
        dbg!(instance.event_count());
        assert_eq!(emit_count.load(std::sync::atomic::Ordering::SeqCst), 3);
        // we expect the listener to be garage colleted  by here
        assert_eq!(instance.listener_count_by_event(&event), 0);
    }

    #[tester::test(flavor = "multi_thread", worker_threads = 8)]
    async fn once_runs_once_under_concurrent_emits() {
        for _ in 0..100 {
            let emitter: TypedEmitter<&str, (), ()> = TypedEmitter::new();
            let calls = Arc::new(AtomicUsize::new(0));
            let counter = calls.clone();
            emitter.once("event", move |_| {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async {}
            });

            let tasks: Vec<_> = (0..8)
                .map(|_| {
                    let emitter = emitter.clone();
                    tokio::spawn(async move { emitter.emit("event", ()).await })
                })
                .collect();
            futures::future::join_all(tasks).await;

            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn concurrent_on_all_installs_one_listener() {
        for _ in 0..100 {
            let emitter: TypedEmitter<&str, (), ()> = TypedEmitter::new();
            let barrier = Arc::new(std::sync::Barrier::new(8));
            let installed = (0..8)
                .map(|_| {
                    let emitter = emitter.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            emitter.on_all(|_| async {})
                        }))
                        .is_ok()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|ok| *ok)
                .count();

            assert_eq!(installed, 1);
        }
    }

    #[tester::test]
    async fn removing_global_listener_returns_its_id() {
        let emitter: TypedEmitter<&str, (), ()> = TypedEmitter::new();
        let id = emitter.on_all(|_| async {});

        assert_eq!(emitter.remove_listener(id), Some(id));
        assert_eq!(emitter.remove_listener(id), None);

        let replacement = emitter.on_all(|_| async {});
        assert_eq!(emitter.remove_listener(replacement), Some(replacement));
    }
}
