use super::variants::{Operation, Resource};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::{array, mem};
use strum::{EnumCount, IntoEnumIterator};

/// The count of `(Resource, Operation)` combinations.
const BUFFER_LENGTH: usize = Resource::COUNT * Operation::COUNT;

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
            inner: Mutex::new(array::from_fn(|_| Vec::new())),
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
    pub fn index(resource: Resource, operation: Operation) -> usize {
        resource as usize * Operation::COUNT + operation as usize
    }

    /// The `EnumIter` derive assigns deterministic sequential IDs to 
    /// enum variant combinations
    /// 
    /// The row-major [EventBuffer::index] can be reversed and the [Resource] 
    /// and [Operation] recovered by indexing into [RESOURCE_OPERATION]
    pub fn reverse_index(index: usize) -> (Resource, Operation) {
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
}

/// Lazily-constructed index mapping 
/// derived from compiler iteration over enum variants
static RESOURCE_OPERATION: LazyLock<Vec<(Resource, Operation)>> = LazyLock::new(|| {
    Resource::iter()
        .flat_map(|r| Operation::iter().map(move |o| (r, o)))
        .collect()
});
