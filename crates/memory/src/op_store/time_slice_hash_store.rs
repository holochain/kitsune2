use kitsune2_api::{K2Error, K2Result};

/// In-memory store for time slice hashes.
///
/// The inner store will look something like this:
/// `[ Block(0, 3), Single(3, [a, b, c]), Block(4, 2), Single(6, [d, e, f]) ]`
///
/// Consecutive sequences of empty hashes are compressed into a single block. A block
/// contains at least 1 hash.
///
/// Gaps are not permitted, so the representation starts out as a single block that spans
/// the entire range of possible slice ids. As hashes are inserted, the block is split around
/// the inserted hash.
#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
pub(super) struct TimeSliceHashStore {
    pub(super) highest_stored_id: Option<u64>,
    inner: Vec<SliceHash>,
}

impl Default for TimeSliceHashStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub enum SliceHash {
    Single { id: u64, hash: bytes::Bytes },
    Block { id: u64, count: u64 },
}

#[cfg(test)]
impl SliceHash {
    pub(crate) fn id(&self) -> u64 {
        match self {
            SliceHash::Single { id, .. } => *id,
            SliceHash::Block { id, .. } => *id,
        }
    }

    pub(crate) fn end(&self) -> u64 {
        self.id()
            + match self {
                SliceHash::Single { .. } => 0,
                SliceHash::Block { count, .. } => *count,
            }
    }
}

impl TimeSliceHashStore {
    pub(super) fn new() -> Self {
        Self {
            highest_stored_id: None,
            inner: vec![SliceHash::Block {
                id: 0,
                count: u64::MAX,
            }],
        }
    }

    /// Insert a hash at the given slice id.
    pub(super) fn insert(
        &mut self,
        slice_id: u64,
        hash: bytes::Bytes,
    ) -> K2Result<()> {
        if !hash.is_empty() {
            match &mut self.highest_stored_id {
                Some(id) if slice_id > *id => {
                    *id = slice_id;
                }
                None => {
                    self.highest_stored_id = Some(slice_id);
                }
                _ => {}
            }
        }

        let mut handled = false;
        for (i, slice_hash) in self.inner.iter().cloned().enumerate() {
            match slice_hash {
                SliceHash::Single { id, .. } => {
                    if id == slice_id {
                        if hash.is_empty() {
                            // This doesn't need to be supported. If we receive an empty hash
                            // for a slice id after we've already stored a non-empty hash for
                            // that slice id, then the caller has done something wrong.

                            return Err(K2Error::other("Cannot overwrite non-empty hash with empty hash"));
                        } else {
                            // We're overwriting any existing value with a new non-empty value.
                            if let SliceHash::Single {
                                hash: existing_hash,
                                ..
                            } = &mut self.inner[i]
                            {
                                *existing_hash = hash;
                            }
                        }

                        handled = true;
                        break;
                    }
                }
                SliceHash::Block { id, count, .. } => {
                    if id <= slice_id && slice_id <= id + count {
                        // Completely contained within a compressed block then either
                        //   - It is empty, so we can just ignore it.
                        //   - It is not empty, so we need to split the block into two.

                        if !hash.is_empty() {
                            if slice_id == id {
                                // Special case for inserting at the start of the block.

                                // We're overwriting the first value in the block.
                                self.inner[i] =
                                    SliceHash::Single { id: slice_id, hash };

                                if count > 1 {
                                    self.inner.insert(
                                        i + 1,
                                        SliceHash::Block {
                                            id: slice_id + 1,
                                            count: count - 1,
                                        },
                                    );
                                }
                            } else if slice_id == id + count {
                                // Special case for inserting at the end of the block.

                                // We're overwriting the last value in the block.
                                self.inner[i] =
                                    SliceHash::Single { id: slice_id, hash };

                                if count > 1 {
                                    self.inner.insert(
                                        i,
                                        SliceHash::Block {
                                            id,
                                            count: count - 1,
                                        },
                                    );
                                }
                            } else {
                                // Otherwise we're inserting in the middle of a block.

                                // Contained within a compressed block but not empty, so we need
                                // to split the block into two.
                                let lower_block = SliceHash::Block {
                                    id,
                                    count: slice_id - id - 1,
                                };
                                let upper_block = SliceHash::Block {
                                    id: slice_id + 1,
                                    count: count - (slice_id - id) - 1,
                                };

                                // We're going to do one overwrite and two inserts.
                                self.inner.reserve(2);
                                self.inner[i] = lower_block;
                                self.inner.insert(
                                    i + 1,
                                    SliceHash::Single { id: slice_id, hash },
                                );
                                self.inner.insert(i + 2, upper_block);
                            }
                        }

                        handled = true;
                        break;
                    }
                }
            }
        }

        if !handled {
            return Err(K2Error::other("Did not know how to insert hash"));
        }

        Ok(())
    }

    pub(super) fn get(&self, slice_id: u64) -> Option<&SliceHash> {
        self.inner.iter().find(|slice_hash| match slice_hash {
            SliceHash::Single { id, .. } => *id == slice_id,
            SliceHash::Block { id, count, .. } => {
                *id <= slice_id && slice_id <= id + count
            }
        })
    }

    #[cfg(test)]
    pub(super) fn check(&self) -> bool {
        let mut highest_stored_id = 0;
        for (i, slice_hash) in self.inner.iter().enumerate() {
            let previous_end = if i > 0 {
                self.inner[i - 1].end()
            } else {
                u64::MAX
            };

            match slice_hash {
                SliceHash::Single { id, .. } => {
                    if *id != previous_end.wrapping_add(1) {
                        println!(
                            "Single does not follow previous: {} != {}",
                            *id,
                            previous_end.wrapping_add(1)
                        );
                        return false;
                    }
                    highest_stored_id = *id;
                }
                SliceHash::Block { id, count, .. } => {
                    if *id != previous_end.wrapping_add(1) {
                        println!(
                            "Block does not follow previous: {} != {}",
                            *id,
                            previous_end.wrapping_add(1)
                        );
                        return false;
                    }

                    if *count > u64::MAX - *id {
                        // Not allowed to wrap around
                        println!("Block wraps around");
                        return false;
                    }
                }
            }
        }

        if highest_stored_id != 0
            && highest_stored_id != self.highest_stored_id.unwrap()
        {
            println!(
                "highest_stored_id does not match: {} != {:?}",
                highest_stored_id, self.highest_stored_id
            );
            return false;
        }

        match self.inner.last() {
            Some(SliceHash::Block { id, count }) => {
                if *count != u64::MAX - id {
                    println!(
                        "Block does not cover remaining range: {} != {}",
                        *count,
                        u64::MAX - highest_stored_id
                    );
                    return false;
                }
            }
            Some(SliceHash::Single { id, .. }) => {
                if *id != highest_stored_id || *id != u64::MAX {
                    println!(
                        "Single does not cover remaining range: {} != {}",
                        *id, highest_stored_id
                    );
                    return false;
                }
            }
            _ => {
                println!("No last element");
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_empty() {
        let store = TimeSliceHashStore::new();

        assert_eq!(None, store.highest_stored_id);
        assert_eq!(1, store.inner.len());
        assert_eq!(
            SliceHash::Block {
                id: 0,
                count: u64::MAX
            },
            store.inner[0]
        );

        assert!(store.check());
    }

    #[test]
    fn insert_empty_hash_into_empty() {
        let mut store = TimeSliceHashStore::new();
        let original = store.clone();

        store.insert(100, bytes::Bytes::new()).unwrap();

        // Doesn't change the stored value, but does update the highest stored value
        assert_eq!(original.inner, store.inner);
        assert_eq!(None, store.highest_stored_id);

        assert!(store.check());
    }

    #[test]
    fn insert_single_hash_into_empty() {
        let mut store = TimeSliceHashStore::new();

        store.insert(100, vec![1, 2, 3].into()).unwrap();

        assert_eq!(3, store.inner.len());
        assert_eq!(SliceHash::Block { id: 0, count: 99 }, store.inner[0]);
        assert_eq!(
            SliceHash::Single {
                id: 100,
                hash: vec![1, 2, 3].into()
            },
            store.inner[1]
        );
        assert_eq!(
            SliceHash::Block {
                id: 101,
                count: u64::MAX - 101
            },
            store.inner[2]
        );
        assert_eq!(Some(100), store.highest_stored_id);

        assert!(store.check());
    }

    #[test]
    fn split_at_right_end_of_block() {
        let mut store = TimeSliceHashStore::new();

        store.insert(100, vec![1, 2, 3].into()).unwrap();
        assert_eq!(3, store.inner.len());

        store.insert(99, vec![2, 3, 4].into()).unwrap();
        assert_eq!(4, store.inner.len());

        assert_eq!(SliceHash::Block { id: 0, count: 98 }, store.inner[0]);
        assert_eq!(
            SliceHash::Single {
                id: 99,
                hash: vec![2, 3, 4].into()
            },
            store.inner[1]
        );
        assert_eq!(
            SliceHash::Single {
                id: 100,
                hash: vec![1, 2, 3].into()
            },
            store.inner[2]
        );
        assert_eq!(
            SliceHash::Block {
                id: 101,
                count: u64::MAX - 101
            },
            store.inner[3]
        );

        assert!(store.check());
    }

    #[test]
    fn split_at_left_end_of_block() {
        let mut store = TimeSliceHashStore::new();

        store.insert(100, vec![1, 2, 3].into()).unwrap();
        assert_eq!(3, store.inner.len());

        store.insert(101, vec![2, 3, 4].into()).unwrap();
        assert_eq!(4, store.inner.len());

        assert_eq!(SliceHash::Block { id: 0, count: 99 }, store.inner[0]);
        assert_eq!(
            SliceHash::Single {
                id: 100,
                hash: vec![1, 2, 3].into()
            },
            store.inner[1]
        );
        assert_eq!(
            SliceHash::Single {
                id: 101,
                hash: vec![2, 3, 4].into()
            },
            store.inner[2]
        );
        assert_eq!(
            SliceHash::Block {
                id: 102,
                count: u64::MAX - 102
            },
            store.inner[3]
        );

        assert!(store.check());
    }

    #[test]
    fn convert_unit_block_into_single_hash() {
        let mut store = TimeSliceHashStore::new();

        store.insert(100, vec![1, 2, 3].into()).unwrap();
        assert_eq!(3, store.inner.len());

        store.insert(102, vec![2, 3, 4].into()).unwrap();
        assert_eq!(5, store.inner.len());

        assert_eq!(SliceHash::Block { id: 101, count: 0 }, store.inner[2]);

        store.insert(101, vec![3, 4, 5].into()).unwrap();
        assert_eq!(5, store.inner.len());

        assert_eq!(
            SliceHash::Single {
                id: 100,
                hash: vec![1, 2, 3].into()
            },
            store.inner[1]
        );
        assert_eq!(
            SliceHash::Single {
                id: 101,
                hash: vec![3, 4, 5].into()
            },
            store.inner[2]
        );
        assert_eq!(
            SliceHash::Single {
                id: 102,
                hash: vec![2, 3, 4].into()
            },
            store.inner[3]
        );

        assert!(store.check());
    }

    #[test]
    fn overwrite_existing_single_hash() {
        let mut store = TimeSliceHashStore::new();

        store.insert(100, vec![1, 2, 3].into()).unwrap();
        assert_eq!(3, store.inner.len());

        assert_eq!(
            SliceHash::Single {
                id: 100,
                hash: vec![1, 2, 3].into()
            },
            store.inner[1]
        );

        store.insert(100, vec![2, 3, 4].into()).unwrap();
        assert_eq!(3, store.inner.len());

        assert_eq!(
            SliceHash::Single {
                id: 100,
                hash: vec![2, 3, 4].into()
            },
            store.inner[1]
        );

        assert!(store.check());
    }

    #[test]
    fn highest_stored_ignores_empty() {
        let mut store = TimeSliceHashStore::new();

        store.insert(100, vec![1, 2, 3].into()).unwrap();
        assert_eq!(Some(100), store.highest_stored_id);

        store.insert(5000, bytes::Bytes::new()).unwrap();
        assert_eq!(Some(100), store.highest_stored_id);

        store.insert(120, vec![2, 3, 4].into()).unwrap();
        assert_eq!(Some(120), store.highest_stored_id);

        assert!(store.check());
    }

    #[test]
    fn get() {
        let mut store = TimeSliceHashStore::new();

        store.insert(100, vec![1, 2, 3].into()).unwrap();

        assert_eq!(
            Some(&SliceHash::Single {
                id: 100,
                hash: vec![1, 2, 3].into()
            }),
            store.get(100)
        );
        assert_eq!(Some(&SliceHash::Block { id: 0, count: 99 }), store.get(0));
        assert_eq!(Some(&SliceHash::Block { id: 0, count: 99 }), store.get(30));
        assert_eq!(Some(&SliceHash::Block { id: 0, count: 99 }), store.get(99));
        assert_eq!(
            Some(&SliceHash::Block {
                id: 101,
                count: u64::MAX - 101
            }),
            store.get(101)
        );
        assert_eq!(
            Some(&SliceHash::Block {
                id: 101,
                count: u64::MAX - 101
            }),
            store.get(500)
        );
        assert_eq!(
            Some(&SliceHash::Block {
                id: 101,
                count: u64::MAX - 101
            }),
            store.get(u64::MAX)
        );
    }
}
