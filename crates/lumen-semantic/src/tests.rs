use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_content::Control;
use lumen_core::CancellationToken;
use lumen_embedding::{Embedder, EmbeddingProfile, MockBackend, MockLatency, dot};

use super::*;

fn mock(per_call_ms: u64) -> MakeEmbedder {
    Box::new(move || {
        Embedder::new(
            Arc::new(MockBackend::with_latency(MockLatency {
                per_call: Duration::from_millis(per_call_ms),
                ..MockLatency::default()
            })),
            EmbeddingProfile::DEFAULT,
        )
        .map_err(|e| e.to_string())
    })
}

fn wait_for(mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn embeds_normalized_vectors_and_caches_them() {
    let q = QueryEmbedder::start(mock(0), None, QueryConfig::default()).unwrap();
    let never = CancellationToken::new();
    let a = q.embed("transformers paper", &never).unwrap();
    assert_eq!(a.len(), 256);
    assert!((dot(&a, &a) - 1.0).abs() < 1e-4);
    let again = q.embed("  transformers paper ", &never).unwrap();
    assert!(Arc::ptr_eq(&a, &again), "cache hit returns the same vector");
    let s = q.stats();
    assert_eq!((s.requests, s.cache_hits, s.embedded), (2, 1, 1));
    assert!(s.warm && s.last_embed.is_some());
    q.clear_cache();
    let b = q.embed("transformers paper", &never).unwrap();
    assert!(!Arc::ptr_eq(&a, &b) && dot(&a, &b) > 0.9999);
}

#[test]
fn newer_queries_supersede_queued_ones() {
    let q = Arc::new(QueryEmbedder::start(mock(120), None, QueryConfig::default()).unwrap());
    let never = CancellationToken::new();
    q.embed("warm up", &never).unwrap();
    let spawn = |text: &'static str| {
        let q = Arc::clone(&q);
        std::thread::spawn(move || q.embed(text, &CancellationToken::new()))
    };
    let first = spawn("tra"); // runs (120 ms)
    std::thread::sleep(Duration::from_millis(30));
    let second = spawn("trans"); // queued
    std::thread::sleep(Duration::from_millis(30));
    let third = spawn("transformer"); // replaces the queued one
    assert!(first.join().unwrap().is_ok());
    assert_eq!(second.join().unwrap(), Err(QueryError::Superseded));
    assert!(third.join().unwrap().is_ok());
    assert_eq!(q.stats().superseded, 1);
}

#[test]
fn a_waiting_caller_can_cancel() {
    let q = Arc::new(QueryEmbedder::start(mock(150), None, QueryConfig::default()).unwrap());
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    let q2 = Arc::clone(&q);
    let waiter = std::thread::spawn(move || q2.embed("slow query", &c2));
    std::thread::sleep(Duration::from_millis(20));
    cancel.cancel();
    let t = Instant::now();
    assert_eq!(waiter.join().unwrap(), Err(QueryError::Cancelled));
    assert!(t.elapsed() < Duration::from_millis(100));
}

#[test]
fn queries_hold_indexing_until_the_linger_ends() {
    let control = Control::new();
    let cfg = QueryConfig {
        linger: Duration::from_millis(150),
        ..QueryConfig::default()
    };
    let q = QueryEmbedder::start(mock(30), Some(control.clone()), cfg).unwrap();
    let never = CancellationToken::new();
    assert!(control.wait_until_runnable(&never, Duration::ZERO));
    q.embed("hello", &never).unwrap();
    // Still lingering right after the query.
    assert!(!control.wait_until_runnable(&never, Duration::ZERO));
    assert!(wait_for(
        || control.wait_until_runnable(&never, Duration::ZERO)
    ));
}

#[test]
fn warm_unload_and_idle_unload() {
    let cfg = QueryConfig {
        idle_unload: Some(Duration::from_millis(80)),
        ..QueryConfig::default()
    };
    let q = QueryEmbedder::start(mock(0), None, cfg).unwrap();
    q.warm();
    assert!(wait_for(|| q.stats().warm));
    q.unload();
    assert!(wait_for(|| !q.stats().warm));
    q.embed("x", &CancellationToken::new()).unwrap();
    assert!(q.stats().warm);
    assert!(wait_for(|| !q.stats().warm), "unloaded after idling");
}

#[test]
fn a_model_that_cannot_load_is_reported() {
    let q = QueryEmbedder::start(
        Box::new(|| Err("model missing".into())),
        None,
        QueryConfig::default(),
    )
    .unwrap();
    let never = CancellationToken::new();
    assert_eq!(
        q.embed("x", &never),
        Err(QueryError::Unavailable("model missing".into()))
    );
    assert!(matches!(
        q.embed("y", &never),
        Err(QueryError::Unavailable(_))
    ));
}
