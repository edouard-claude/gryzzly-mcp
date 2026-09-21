//! Small time-to-live caches, so resolving a project name does not cost a
//! request every time.
//!
//! Nothing here is clever: a value, the instant it was read, and a TTL. The
//! point is that `log_time` called five times in a row for five days of a week
//! reads the project list once.

use std::{collections::HashMap, hash::Hash, sync::Arc, time::Duration};

use tokio::{sync::RwLock, time::Instant};

/// One cached value with an expiry.
#[derive(Debug)]
pub struct TtlCell<T> {
    ttl: Duration,
    slot: RwLock<Option<(Instant, Arc<T>)>>,
}

impl<T> TtlCell<T> {
    /// A cell that forgets its value after `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            slot: RwLock::new(None),
        }
    }

    /// The value, when it is there and still fresh.
    pub async fn get(&self) -> Option<Arc<T>> {
        let slot = self.slot.read().await;
        slot.as_ref()
            .and_then(|(at, value)| (at.elapsed() < self.ttl).then(|| Arc::clone(value)))
    }

    /// Store a value and hand back the shared handle.
    pub async fn set(&self, value: T) -> Arc<T> {
        let shared = Arc::new(value);
        *self.slot.write().await = Some((Instant::now(), Arc::clone(&shared)));
        shared
    }

    /// Forget the value, which a write does to what it just changed.
    pub async fn clear(&self) {
        *self.slot.write().await = None;
    }
}

/// The same, keyed.
#[derive(Debug)]
pub struct TtlMap<K, V> {
    ttl: Duration,
    slots: RwLock<HashMap<K, (Instant, Arc<V>)>>,
}

impl<K: Eq + Hash, V> TtlMap<K, V> {
    /// A map whose entries expire after `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            slots: RwLock::new(HashMap::new()),
        }
    }

    /// The value for `key`, when it is there and still fresh.
    pub async fn get(&self, key: &K) -> Option<Arc<V>> {
        let slots = self.slots.read().await;
        slots
            .get(key)
            .and_then(|(at, value)| (at.elapsed() < self.ttl).then(|| Arc::clone(value)))
    }

    /// Store a value for `key` and hand back the shared handle.
    pub async fn set(&self, key: K, value: V) -> Arc<V> {
        let shared = Arc::new(value);
        self.slots
            .write()
            .await
            .insert(key, (Instant::now(), Arc::clone(&shared)));
        shared
    }

    /// Forget everything.
    pub async fn clear(&self) {
        self.slots.write().await.clear();
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_cell_forgets_its_value_after_the_ttl() {
        let cell = TtlCell::new(Duration::from_secs(60));
        cell.set(vec![1, 2, 3]).await;
        assert!(cell.get().await.is_some());
        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(cell.get().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn clearing_a_cell_is_what_a_write_does() {
        let cell = TtlCell::new(Duration::from_secs(60));
        cell.set(1_u8).await;
        cell.clear().await;
        assert!(cell.get().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn map_entries_expire_independently() {
        let map: TtlMap<String, u8> = TtlMap::new(Duration::from_secs(30));
        map.set("a".into(), 1).await;
        tokio::time::advance(Duration::from_secs(20)).await;
        map.set("b".into(), 2).await;
        tokio::time::advance(Duration::from_secs(15)).await;
        assert!(map.get(&"a".to_owned()).await.is_none());
        assert!(map.get(&"b".to_owned()).await.is_some());
    }
}
