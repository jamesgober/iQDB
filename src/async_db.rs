// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! The async surface (`async` feature).
//!
//! The iqdb family is synchronous by design — a nearest-neighbour search is a
//! CPU-bound scan or graph walk, and a durable write is a blocking `fsync`,
//! not async I/O. Wrapping that work in a future buys nothing and blocking it
//! on the executor thread would stall every other task on the runtime.
//!
//! [`AsyncIqdb`] is therefore a thin Tokio adapter, not a re-implementation:
//! it holds an `Arc<Iqdb>` and runs each blocking call on Tokio's dedicated
//! blocking pool via [`tokio::task::spawn_blocking`], so the executor thread
//! is never blocked. The synchronous [`Iqdb`](crate::Iqdb) API is unchanged
//! and remains the source of truth; everything here delegates to it.
//!
//! Cheap, non-blocking accessors (`len`, `dim`, `metric`, …) stay synchronous
//! — there is nothing to offload.

use std::path::Path;
use std::sync::Arc;

use tokio::task::JoinError;

use crate::config::IqdbConfig;
use crate::error::Result;
use crate::{CacheStats, DistanceMetric, Filter, Hit, Iqdb, Metadata, Vector, VectorId};

/// An async handle over an [`Iqdb`](crate::Iqdb) database.
///
/// Every fallible / blocking operation is offloaded to Tokio's blocking pool,
/// so awaiting a search or a write never stalls the executor. Construct one
/// with [`AsyncIqdb::open_in_memory`] or [`AsyncIqdb::open`]; it is `Clone`
/// (cheap — it shares the underlying handle through an `Arc`) and
/// `Send + Sync`.
///
/// # Examples
///
/// ```
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// use iqdb::{AsyncIqdb, DistanceMetric, Vector, VectorId};
///
/// let db = AsyncIqdb::open_in_memory(3, DistanceMetric::Cosine).await?;
/// db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0, 0.0])?, None).await?;
/// db.upsert(VectorId::from(2u64), Vector::new(vec![0.0, 1.0, 0.0])?, None).await?;
///
/// let hits = db.search(Vector::new(vec![1.0, 0.0, 0.0])?, 1).await?;
/// assert_eq!(hits[0].id, VectorId::from(1u64));
/// db.close().await?;
/// # Ok::<(), iqdb::Error>(())
/// # }).unwrap();
/// ```
#[derive(Debug, Clone)]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub struct AsyncIqdb {
    inner: Arc<Iqdb>,
}

impl AsyncIqdb {
    /// Open an ephemeral, in-memory database (exact flat index).
    ///
    /// In-memory construction does not touch the filesystem, so this resolves
    /// without offloading to the blocking pool.
    ///
    /// # Errors
    ///
    /// [`Error::Config`](crate::Error::Config) if `dim` is zero.
    pub async fn open_in_memory(dim: usize, metric: DistanceMetric) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Iqdb::open_in_memory(dim, metric)?),
        })
    }

    /// Open an in-memory database from a full [`IqdbConfig`].
    ///
    /// # Errors
    ///
    /// As [`Iqdb::open_in_memory_with`](crate::Iqdb::open_in_memory_with).
    pub async fn open_in_memory_with(config: IqdbConfig) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Iqdb::open_in_memory_with(config)?),
        })
    }

    /// Open or create a durable, file-backed database at `path`. The open
    /// (snapshot load + WAL replay) runs on the blocking pool.
    ///
    /// # Errors
    ///
    /// As [`Iqdb::open`](crate::Iqdb::open).
    pub async fn open<P: AsRef<Path>>(path: P, dim: usize, metric: DistanceMetric) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let inner =
            unwrap_join(tokio::task::spawn_blocking(move || Iqdb::open(path, dim, metric)).await)?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Open or create a durable, file-backed database from a full
    /// [`IqdbConfig`].
    ///
    /// # Errors
    ///
    /// As [`Iqdb::open_with`](crate::Iqdb::open_with).
    pub async fn open_with<P: AsRef<Path>>(path: P, config: IqdbConfig) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let inner =
            unwrap_join(tokio::task::spawn_blocking(move || Iqdb::open_with(path, config)).await)?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Insert or replace the record under `id`. See [`Iqdb::upsert`](crate::Iqdb::upsert).
    ///
    /// # Errors
    ///
    /// As [`Iqdb::upsert`](crate::Iqdb::upsert).
    pub async fn upsert(
        &self,
        id: VectorId,
        vector: Vector,
        metadata: Option<Metadata>,
    ) -> Result<()> {
        let db = Arc::clone(&self.inner);
        unwrap_join(tokio::task::spawn_blocking(move || db.upsert(id, vector, metadata)).await)
    }

    /// Look up the stored vector and metadata for `id`. See [`Iqdb::get`](crate::Iqdb::get).
    ///
    /// # Errors
    ///
    /// As [`Iqdb::get`](crate::Iqdb::get).
    pub async fn get(&self, id: VectorId) -> Result<Option<(Vector, Option<Metadata>)>> {
        let db = Arc::clone(&self.inner);
        unwrap_join(tokio::task::spawn_blocking(move || db.get(&id)).await)
    }

    /// Delete the record under `id`. See [`Iqdb::delete`](crate::Iqdb::delete).
    ///
    /// # Errors
    ///
    /// As [`Iqdb::delete`](crate::Iqdb::delete).
    pub async fn delete(&self, id: VectorId) -> Result<bool> {
        let db = Arc::clone(&self.inner);
        unwrap_join(tokio::task::spawn_blocking(move || db.delete(&id)).await)
    }

    /// Top-`k` similarity search. See [`Iqdb::search`](crate::Iqdb::search).
    ///
    /// Takes the query by value because the work runs on another thread; clone
    /// the query if you need to keep it.
    ///
    /// # Errors
    ///
    /// As [`Iqdb::search`](crate::Iqdb::search).
    ///
    /// # Examples
    ///
    /// ```
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// use iqdb::{AsyncIqdb, DistanceMetric, Vector, VectorId};
    ///
    /// let db = AsyncIqdb::open_in_memory(2, DistanceMetric::Euclidean).await?;
    /// db.upsert(VectorId::from(1u64), Vector::new(vec![0.0, 0.0])?, None).await?;
    /// db.upsert(VectorId::from(2u64), Vector::new(vec![9.0, 9.0])?, None).await?;
    ///
    /// let hits = db.search(Vector::new(vec![0.1, 0.1])?, 1).await?;
    /// assert_eq!(hits[0].id, VectorId::from(1u64));
    /// # Ok::<(), iqdb::Error>(())
    /// # }).unwrap();
    /// ```
    pub async fn search(&self, query: Vector, k: usize) -> Result<Vec<Hit>> {
        let db = Arc::clone(&self.inner);
        unwrap_join(tokio::task::spawn_blocking(move || db.search(&query, k)).await)
    }

    /// Top-`k` search filtered by metadata. See [`Iqdb::search_with`](crate::Iqdb::search_with).
    ///
    /// # Errors
    ///
    /// As [`Iqdb::search_with`](crate::Iqdb::search_with).
    pub async fn search_with(&self, query: Vector, k: usize, filter: Filter) -> Result<Vec<Hit>> {
        let db = Arc::clone(&self.inner);
        unwrap_join(tokio::task::spawn_blocking(move || db.search_with(&query, k, filter)).await)
    }

    /// Batch search, one result list per query. See [`Iqdb::search_batch`](crate::Iqdb::search_batch).
    ///
    /// # Errors
    ///
    /// As [`Iqdb::search_batch`](crate::Iqdb::search_batch).
    pub async fn search_batch(&self, queries: Vec<Vector>, k: usize) -> Result<Vec<Vec<Hit>>> {
        let db = Arc::clone(&self.inner);
        unwrap_join(tokio::task::spawn_blocking(move || db.search_batch(&queries, k)).await)
    }

    /// Batch search with a shared filter. See [`Iqdb::search_batch_with`](crate::Iqdb::search_batch_with).
    ///
    /// # Errors
    ///
    /// As [`Iqdb::search_batch_with`](crate::Iqdb::search_batch_with).
    pub async fn search_batch_with(
        &self,
        queries: Vec<Vector>,
        k: usize,
        filter: Filter,
    ) -> Result<Vec<Vec<Hit>>> {
        let db = Arc::clone(&self.inner);
        unwrap_join(
            tokio::task::spawn_blocking(move || db.search_batch_with(&queries, k, filter)).await,
        )
    }

    /// Rebuild / retrain the approximate index. See [`Iqdb::optimize`](crate::Iqdb::optimize).
    ///
    /// # Errors
    ///
    /// As [`Iqdb::optimize`](crate::Iqdb::optimize).
    pub async fn optimize(&self) -> Result<()> {
        let db = Arc::clone(&self.inner);
        unwrap_join(tokio::task::spawn_blocking(move || db.optimize()).await)
    }

    /// Flush pending writes to durable storage. See [`Iqdb::flush`](crate::Iqdb::flush).
    ///
    /// # Errors
    ///
    /// As [`Iqdb::flush`](crate::Iqdb::flush).
    pub async fn flush(&self) -> Result<()> {
        let db = Arc::clone(&self.inner);
        unwrap_join(tokio::task::spawn_blocking(move || db.flush()).await)
    }

    /// Close the database, compacting a file-backed store one final time.
    /// Consumes the handle.
    ///
    /// If other clones of this `AsyncIqdb` are still alive, the underlying
    /// handle cannot be consumed, so this degrades to a final
    /// [`flush`](Self::flush) and releases this clone.
    ///
    /// # Errors
    ///
    /// As [`Iqdb::close`](crate::Iqdb::close).
    pub async fn close(self) -> Result<()> {
        let db = self.inner;
        unwrap_join(
            tokio::task::spawn_blocking(move || match Arc::try_unwrap(db) {
                Ok(handle) => handle.close(),
                Err(shared) => shared.flush(),
            })
            .await,
        )
    }

    /// The fixed dimensionality.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.inner.dim()
    }

    /// The fixed distance metric.
    #[must_use]
    pub fn metric(&self) -> DistanceMetric {
        self.inner.metric()
    }

    /// Number of stored vectors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// `true` if no vectors are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Cache hit/miss statistics, or `None` when uncached.
    #[must_use]
    pub fn cache_stats(&self) -> Option<CacheStats> {
        self.inner.cache_stats()
    }
}

/// Resolve a `spawn_blocking` join result, re-raising a panic from the
/// blocking closure on the calling task. `spawn_blocking` tasks are not
/// cancellable, so a [`JoinError`] here always carries a panic.
fn unwrap_join<T>(res: std::result::Result<T, JoinError>) -> T {
    match res {
        Ok(value) => value,
        Err(join) => std::panic::resume_unwind(join.into_panic()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec2(a: f32, b: f32) -> Vector {
        Vector::new(vec![a, b]).unwrap()
    }

    #[tokio::test]
    async fn async_crud_round_trip() {
        let db = AsyncIqdb::open_in_memory(2, DistanceMetric::Euclidean)
            .await
            .unwrap();
        assert!(db.is_empty());
        db.upsert(VectorId::from(1u64), vec2(0.0, 0.0), None)
            .await
            .unwrap();
        db.upsert(VectorId::from(2u64), vec2(3.0, 4.0), None)
            .await
            .unwrap();
        assert_eq!(db.len(), 2);

        let (got, _) = db.get(VectorId::from(2u64)).await.unwrap().unwrap();
        assert_eq!(got.as_slice(), &[3.0, 4.0]);

        let hits = db.search(vec2(0.1, 0.1), 1).await.unwrap();
        assert_eq!(hits[0].id, VectorId::from(1u64));

        assert!(db.delete(VectorId::from(1u64)).await.unwrap());
        assert_eq!(db.len(), 1);
        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn async_handle_is_send_sync_and_cloneable() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AsyncIqdb>();

        let db = AsyncIqdb::open_in_memory(2, DistanceMetric::Cosine)
            .await
            .unwrap();
        let clone = db.clone();
        db.upsert(VectorId::from(1u64), vec2(1.0, 0.0), None)
            .await
            .unwrap();
        // The clone observes the same shared state.
        assert_eq!(clone.len(), 1);
    }

    #[tokio::test]
    async fn async_rejects_zero_dim() {
        assert!(
            AsyncIqdb::open_in_memory(0, DistanceMetric::Cosine)
                .await
                .is_err()
        );
    }
}
