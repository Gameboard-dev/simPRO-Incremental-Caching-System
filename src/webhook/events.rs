use super::variants::{Operation, Resource};
use anyhow::Context;
use std::path::Path;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::{array, mem};
use strum::{EnumCount, IntoEnumIterator};

/// The count of `(Resource, Operation)` combinations.
const BUFFER_LENGTH: usize =
    Resource::COUNT * Operation::COUNT;

/// An array of record IDs indexed by Resource and Operation:
pub type Buffer = [Vec<u64>; BUFFER_LENGTH];

#[derive(Debug)]
/// Thread-safe buffer containing record IDs pending retrieval.
/// The internal [`Buffer`] contains one `Vec<u64>` for every
/// ([`Resource`], [`Operation`]) combination.
pub struct EventBuffer {
    inner: Mutex<Buffer>,
}

impl Default for EventBuffer {
    /// Creates an empty buffer with one IDs array
    /// per ([`Resource`], [`Operation`]) combination.
    ///
    /// Allocation size is fixed based on
    /// Resource::COUNT * Operation::COUNT.
    fn default() -> Self {
        Self {
            inner: Mutex::new(array::from_fn(|_| {
                Vec::new()
            })),
        }
    }
}

impl EventBuffer {
    /// Acquires exclusive mutable access to the internal [`Buffer`].
    ///
    /// Returns a [`MutexGuard`] which holds the mutex lock for the
    /// duration of its lifetime and provides mutable access
    /// to the protected buffer.
    ///
    /// Poisoned locks are recovered by returning the contained (inner) value
    /// instead of propagating the poisoning error.
    pub fn acquire_lock(&self) -> MutexGuard<'_, Buffer> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Computes the Row-Major index of a `Resource` and `Operation` combination.
    ///
    /// This depends on [`Resource`] and [`Operation`] having stable integer
    /// discriminants via `#[repr(u8)]`.
    ///
    /// ```rust,ignore
    /// index = (resource as usize) * Operation::COUNT + (operation as usize)
    /// ```
    #[inline]
    pub fn index(
        resource: Resource,
        operation: Operation,
    ) -> usize {
        resource as usize * Operation::COUNT
            + operation as usize
    }

    /// The `EnumIter` derive assigns deterministic sequential IDs to
    /// enum variant combinations
    ///
    /// The row-major [EventBuffer::index] can be reversed and the [Resource]
    /// and [Operation] recovered by indexing into [RESOURCE_OPERATION]
    pub fn reverse_index(
        index: usize,
    ) -> (Resource, Operation) {
        RESOURCE_OPERATION[index]
    }

    /// Atomically drains the entire record ID buffer, returning all
    /// record IDs indexed by `Resource` and `Operation`
    ///
    /// ```rust,ignore
    /// mem::take(&mut *self.acquire_lock())
    /// ```
    ///
    /// This operation acquires exclusive mutex access over the entire
    /// buffer for the duration of the drain.
    ///
    /// `mem::take` replaces the internal buffer with empty vectors and
    /// returns the previous buffer without cloning its contents.
    pub fn drain(&self) -> Buffer {
        mem::take(&mut *self.acquire_lock())
    }

    pub fn new(buffer: Buffer) -> Self {
        Self {
            inner: Mutex::new(buffer),
        }
    }

    pub fn snapshot(&self) -> Buffer {
        self.acquire_lock().clone()
    }

    /// Uses [`serde_json`] to load a previously-persisted 
    /// [`EventBuffer`] snapshot from disk into memory.
    ///
    /// If the file does not exist, an empty default buffer 
    /// is returned instead.
    pub fn load_from_file(
        path: &Path,
    ) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let bytes =
            std::fs::read(path).with_context(|| {
                format!("failed to read {}", path.display())
            })?;

        let buffer: Buffer = serde_json::from_slice(&bytes)
            .with_context(|| {
                format!(
                    "failed to parse {}",
                    path.display()
                )
            })?;

        Ok(Self::from_buffer(buffer))
    }

    /// Constructs a thread-safe [`EventBuffer`] wrapper around an existing
    /// raw [`Buffer`] snapshot.
    ///
    /// [`Buffer`] is the plain in-memory array representation, while
    /// [`EventBuffer`] adds thread-safe concurrent access through a [`Mutex`].
    pub fn from_buffer(buffer: Buffer) -> Self {
        Self::new(buffer)
    }

    /// Atomically writes a [`Buffer`] snapshot to disk as JSON.
    ///
    /// Data is first written to a temporary file and flushed to the
    /// operating system using `sync_all()` before being renamed over
    /// the destination path. This minimizes the risk of partially-written
    /// or truncated persistence files after crashes or abrupt shutdowns.
    ///
    pub(crate) fn persist_to_file(
        &self,
        path: &Path,
    ) -> anyhow::Result<()> {
        let buffer = self.acquire_lock().clone();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp_path = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&buffer)?;
        {
            use std::fs::File;
            use std::io::Write;
            let mut file = File::create(&tmp_path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// While synchronization is running, new webhook events may still arrive.
    /// This method removes only the IDs that were part of the completed sync
    /// snapshot while preserving newly-buffered events.
    ///
    /// ```ignore
    /// snapshot = [1, 2, 3]
    /// live     = [1, 2, 3, 4]
    /// result   = [4]
    /// ```
    ///
    /// The synchronized IDs `[1, 2, 3]` are removed while newly-arrived
    /// event `4` remains buffered for the next synchronization cycle.
    pub fn remove_synced(&self, synced: &Buffer) {
        let mut buffer = self.acquire_lock();

        for (current_ids, synced_ids) in
            buffer.iter_mut().zip(synced.iter())
        {
            if synced_ids.is_empty() {
                continue;
            }
            let synced_ids = synced_ids
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>(
            );
            current_ids
                .retain(|id| !synced_ids.contains(id));
        }
    }
}

/// Lazily-constructed index mapping
/// derived from compiler iteration over enum variants
static RESOURCE_OPERATION: LazyLock<
    Vec<(Resource, Operation)>,
> = LazyLock::new(|| {
    Resource::iter()
        .flat_map(|r| {
            Operation::iter().map(move |o| (r, o))
        })
        .collect()
});
