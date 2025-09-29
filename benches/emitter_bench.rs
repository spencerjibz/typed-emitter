#[macro_use]
extern crate criterion;
use criterion::BenchmarkId;
use criterion::Criterion;
use std::future::Future;
use std::hint::black_box;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::broadcast;
use typed_emitter::TypedEmitter as NewEmitter;
use typed_emitter_old::TypedEmitter as OldEmitter;
use uuid::Uuid;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Event {
    Hello,
    Spam,
    Ping,
}

#[derive(Clone, Copy, Debug)]
enum Payload {
    One,
    Two,
    Many(u32),
}
#[derive(Clone, Debug)]
enum Emitter<Key, Payload> {
    Old(OldEmitter<Key, Payload>),
    New(NewEmitter<Key, Payload>),
}
impl<Key: Clone + Send + Ord + std::hash::Hash, Payload: Clone + 'static> Emitter<Key, Payload> {
    async fn emit(&self, key: Key, payload: Payload) {
        match self {
            Emitter::Old(ref emitter) => emitter.emit(key, payload).await,
            Emitter::New(ref new) => new.emit(key, payload).await,
        }
    }

    pub fn on<F, C>(&self, event: Key, callback: C) -> Uuid
    where
        C: Fn(Payload) -> F + Send + Sync + 'static,
        F: Future<Output = ()> + Send + Sync + 'static,
    {
        match self {
            Emitter::Old(old) => old.on(event, callback),
            Emitter::New(new) => new.on(event, callback),
        }
    }
}

// --- helpers ---

async fn add_and_emit(
    mut emitter: Emitter<Event, Payload>,
    key: Event,
    n_listeners: usize,
    payload: Payload,
) {
    for _ in 0..n_listeners {
        emitter.on(key, |_| async move {});
    }
    emitter.emit(key, payload).await;
}

async fn throughput(emitter: Emitter<Event, Payload>, key: Event, n: u32) {
    let _ = emitter.on(key, |_| async move {});
    for i in 0..n {
        emitter.emit(key, black_box(Payload::Many(i))).await;
    }
}
async fn throughput_parallel(emitter: Emitter<Event, Payload>, tasks: usize, n_each: u32) {
    emitter.on(Event::Ping, |_| async move {});

    let mut handles = Vec::new();
    for _ in 0..tasks {
        let e = emitter.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..n_each {
                e.emit(Event::Ping, Payload::Many(i)).await;
            }
        }));
    }
    futures::future::join_all(handles).await;
}

async fn broadcast_add_and_emit(n_listeners: usize, payload: Payload) {
    let (tx, rx) = broadcast::channel::<Payload>(16);

    for _ in 0..n_listeners {
        let mut r = tx.subscribe();
        tokio::spawn(async move {
            let _ = r.recv().await;
        });
    }

    let _ = tx.send(payload);
}

async fn broadcast_throughput(n: u32) {
    let (tx, mut rx) = broadcast::channel::<Payload>(16);
    tokio::spawn(async move { while let Ok(_msg) = rx.recv().await {} });
    for i in 0..n {
        let _ = tx.send(black_box(Payload::Many(i)));
    }
}
async fn throughput_broadcast_parallel(tasks: usize, n_each: u32) {
    let (tx, mut rx) = broadcast::channel::<Payload>(1024);
    tokio::spawn(async move { while let Ok(_msg) = rx.recv().await {} });
    let mut handles = Vec::new();
    for _ in 0..tasks {
        let txc = tx.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..n_each {
                let _ = txc.send(Payload::Many(i));
            }
        }));
    }
    futures::future::join_all(handles).await;
}

fn bench_multiple_emits_parallel(c: &mut Criterion) {
    let mut group = c.benchmark_group("emit 100k events  parallel");
    let rt = Builder::new_multi_thread()
        .enable_all()
        .worker_threads(num_cpus::get())
        .build()
        .unwrap();

    group.bench_function("old Emitter", |b| {
        b.to_async(&rt).iter(|| {
            let emitter = OldEmitter::<Event, Payload, ()>::new();
            throughput_parallel(Emitter::Old(emitter), 4, 100000)
        });
    });

    group.bench_function("new Emitter", |b| {
        b.to_async(&rt).iter(|| {
            let emitter = NewEmitter::<Event, Payload, ()>::new();
            throughput_parallel(Emitter::New(emitter), 4, 100000)
        });
    });

    group.bench_function(" Tokio broadcast ", |b| {
        b.to_async(&rt)
            .iter(|| throughput_broadcast_parallel(4, 100000));
    });
    group.finish();
}
fn bench_multiple_emits(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("emit a single event");
    group.bench_function("old Emitter", |b| {
        b.to_async(&rt).iter(|| {
            let emitter = OldEmitter::<Event, Payload, ()>::new();
            throughput(Emitter::Old(emitter), Event::Ping, 100000)
        });
    });

    group.bench_function("new Emitter", |b| {
        b.to_async(&rt).iter(|| {
            let emitter = NewEmitter::<Event, Payload, ()>::new();
            throughput(Emitter::New(emitter), Event::Ping, 100000)
        });
    });

    group.bench_function(" Tokio broadcast ", |b| {
        b.to_async(&rt).iter(|| broadcast_throughput(100000));
    });
    group.finish();
}

fn bench_emit_single(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("emit a single event");
    group.bench_function("old Emitter", |b| {
        b.to_async(&rt).iter(|| {
            let emitter = OldEmitter::<Event, Payload, ()>::new();
            add_and_emit(
                Emitter::Old(emitter),
                Event::Hello,
                1,
                black_box(Payload::One),
            )
        });
    });

    group.bench_function("new Emitter", |b| {
        b.to_async(&rt).iter(|| {
            let emitter = NewEmitter::<Event, Payload, ()>::new();
            add_and_emit(
                Emitter::New(emitter),
                Event::Hello,
                1,
                black_box(Payload::One),
            )
        });
    });

    group.bench_function("tokio broadcast", |b| {
        b.to_async(&rt)
            .iter(|| broadcast_add_and_emit(1, black_box(Payload::One)));
    });
    group.finish();
}

fn bench_multiple_listeners(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("emit to 10k listeners");
    group.bench_function("Old Emitter (DashMap based)", |b| {
        b.to_async(&rt).iter(|| {
            let emitter = OldEmitter::<Event, Payload, ()>::new();
            add_and_emit(
                Emitter::Old(emitter),
                Event::Spam,
                10000,
                black_box(Payload::Two),
            )
        });
    });

    group.bench_function("New Emitter", |b| {
        b.to_async(&rt).iter(|| {
            let emitter = NewEmitter::<Event, Payload, ()>::new();
            add_and_emit(
                Emitter::New(emitter),
                Event::Spam,
                10000,
                black_box(Payload::Two),
            )
        });
    });

    group.bench_function("Tokio broadcast", |b| {
        b.to_async(&rt)
            .iter(|| broadcast_add_and_emit(10000, black_box(Payload::Two)));
    });
    group.finish();
}
criterion_group!(
    benches,
    bench_emit_single,
    bench_multiple_listeners,
    bench_multiple_emits_parallel
);
criterion_main!(benches);
