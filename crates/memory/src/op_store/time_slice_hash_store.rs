use kitsune2_api::{K2Error, K2Result};
use std::collections::BTreeMap;

/// In-memory store for time slice hashes.
///
/// Empty hashes do not need to be stored, and will be rejected with an error if you try to insert
/// one. Hashes that are stored, are stored sparsely, and are indexed by the slice id.
///
/// It is valid to look up a time slice which has not had a hash stored, and you will get a `None`
/// response. Otherwise, you will get exactly what was most recently stored for that slice id.
#[derive(Debug, Default)]
#[cfg_attr(test, derive(Clone))]
pub(super) struct TimeSliceHashStore {
    inner: BTreeMap<u64, bytes::Bytes>,
}

impl TimeSliceHashStore {
    /// Insert a hash at the given slice id.
    pub(super) fn insert(
        &mut self,
        slice_id: u64,
        hash: bytes::Bytes,
    ) -> K2Result<()> {
        // This doesn't need to be supported. If we receive an empty hash
        // for a slice id after we've already stored a non-empty hash for
        // that slice id, then the caller has done something wrong.
        // Alternatively, if we've computed an empty hash for a time slice, then we don't
        // need to store that.
        if hash.is_empty() {
            return Err(K2Error::other("Cannot insert empty combined hash"));
        }

        self.inner.insert(slice_id, hash);

        Ok(())
    }

    pub(super) fn get(&self, slice_id: u64) -> Option<bytes::Bytes> {
        self.inner.get(&slice_id).cloned()
    }

    pub fn highest_stored_id(&self) -> Option<u64> {
        self.inner.iter().last().map(|(id, _)| *id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_empty() {
        let store = TimeSliceHashStore::default();

        assert_eq!(None, store.highest_stored_id());
        assert!(store.inner.is_empty());
    }

    #[test]
    fn insert_empty_hash_into_empty() {
        let mut store = TimeSliceHashStore::default();

        let e = store.insert(100, bytes::Bytes::new()).unwrap_err();
        assert_eq!(
            "Cannot insert empty combined hash (src: None)",
            e.to_string()
        );
    }

    #[test]
    fn insert_single_hash_into_empty() {
        let mut store = TimeSliceHashStore::default();

        store.insert(100, vec![1, 2, 3].into()).unwrap();

        assert_eq!(1, store.inner.len());
        assert_eq!(
            bytes::Bytes::from_static(&[1, 2, 3]),
            store.get(100).unwrap()
        );
        assert_eq!(Some(100), store.highest_stored_id());
    }

    #[test]
    fn insert_many_sparse() {
        let mut store = TimeSliceHashStore::default();

        store.insert(100, vec![1, 2, 3].into()).unwrap();
        store.insert(105, vec![2, 3, 4].into()).unwrap();
        store.insert(115, vec![3, 4, 5].into()).unwrap();

        assert_eq!(3, store.inner.len());
        assert_eq!(
            bytes::Bytes::from_static(&[1, 2, 3]),
            store.get(100).unwrap()
        );
        assert_eq!(
            bytes::Bytes::from_static(&[2, 3, 4]),
            store.get(105).unwrap()
        );
        assert_eq!(
            bytes::Bytes::from_static(&[3, 4, 5]),
            store.get(115).unwrap()
        );
        assert_eq!(Some(115), store.highest_stored_id());
    }

    #[test]
    fn insert_many_in_sequence() {
        let mut store = TimeSliceHashStore::default();

        store.insert(100, vec![1, 2, 3].into()).unwrap();
        store.insert(101, vec![2, 3, 4].into()).unwrap();
        store.insert(102, vec![3, 4, 5].into()).unwrap();

        assert_eq!(3, store.inner.len());

        assert_eq!(
            bytes::Bytes::from_static(&[1, 2, 3]),
            store.get(100).unwrap()
        );
        assert_eq!(
            bytes::Bytes::from_static(&[2, 3, 4]),
            store.get(101).unwrap()
        );
        assert_eq!(
            bytes::Bytes::from_static(&[3, 4, 5]),
            store.get(102).unwrap()
        );
        assert_eq!(Some(102), store.highest_stored_id());
    }

    #[test]
    fn overwrite_existing_hash() {
        let mut store = TimeSliceHashStore::default();

        store.insert(100, vec![1, 2, 3].into()).unwrap();
        assert_eq!(1, store.inner.len());

        store.insert(100, vec![2, 3, 4].into()).unwrap();
        assert_eq!(1, store.inner.len());
    }
}
