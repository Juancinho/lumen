//! Stopping background work cooperatively.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared flag a long task checks between steps; cloning shares the same flag.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub async fn crawl(urls: Vec<String>, token: CancelToken) {
    for url in urls {
        if token.is_cancelled() {
            break; // the user typed a new query: stop early
        }
        fetch(&url).await;
    }
}
