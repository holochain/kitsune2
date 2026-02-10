use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::store::*;

/// The space identifier.
pub type SpaceId = bytes::Bytes;

/// A map of spaces.
#[derive(Clone)]
pub struct SpaceMap(Arc<Mutex<HashMap<SpaceId, Space>>>);

impl Default for SpaceMap {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }
}

impl SpaceMap {
    /// Read the content of a space.
    pub fn read(&self, space_id: &SpaceId) -> std::io::Result<Vec<u8>> {
        // minimize outer mutex lock time
        let space = self.0.lock().unwrap().get(space_id).cloned();

        match space {
            Some(space) => space.read(),
            None => Ok(b"[]".to_vec()),
        }
    }

    /// Update all the spaces stored in this map.
    pub fn update_all(&self, max_entries: usize) {
        // minimize outer mutex lock time
        let all = self.0.lock().unwrap().keys().cloned().collect::<Vec<_>>();

        for space_id in all {
            self.update(max_entries, space_id, None);
        }
    }

    /// Update a space, clearing out any expired infos and optionally
    /// adding a new incoming info. If adding a new info, perform all
    /// validation before calling this function.
    pub fn update(
        &self,
        max_entries: usize,
        space_id: SpaceId,
        new_info: Option<(crate::ParsedEntry, Option<StoreEntryRef>)>,
    ) {
        // minimize outer mutex lock time
        let space: Space = {
            use std::collections::hash_map::Entry;

            let mut map = self.0.lock().unwrap();

            if new_info.is_none() && !map.contains_key(&space_id) {
                // we don't have this space, and we're not
                // adding an info to it... nothing to do
                return;
            }

            match map.entry(space_id) {
                Entry::Occupied(e) => {
                    // most other naive methods for clearing out dead spaces
                    // result in either races or deadlock, or they require
                    // us to hold the outer mutex lock for an unacceptably
                    // long time. So, just doing this for now.
                    if new_info.is_none()
                        && e.get().readable.lock().unwrap().is_empty()
                    {
                        e.remove();
                        return;
                    }

                    e.get().clone()
                }
                Entry::Vacant(e) => e.insert(Space::default()).clone(),
            }
        };

        // do the actual update without the outer mutex locked
        space.update(max_entries, new_info);
    }
}

/// A concurrent single space.
#[derive(Clone)]
struct Space {
    // Using a separate write lock instead of a RwLock, because
    // we still want to allow reads while the write_lock is locked.
    // The write function will lock the readable when it is done processing
    // and quickly update the reader, then drop both locks (read first).
    write_lock: Arc<Mutex<()>>,

    // This list of store refs can be cheaply and quickly cloned,
    // minimizing the mutex lock time. Then the actual data content can be
    // read without blocking other threads.
    readable: Arc<Mutex<Vec<StoreEntryRef>>>,
}

impl Default for Space {
    fn default() -> Self {
        Self {
            write_lock: Arc::new(Mutex::new(())),
            readable: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Space {
    /// Read the content of this space.
    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        // keep the mutex lock time to a minimum
        let list = self.readable.lock().unwrap().clone();

        let mut len: usize = list.iter().map(StoreEntryRef::len).sum();

        // plus square brackets
        len += 2;

        // plus commas
        if !list.is_empty() {
            len += list.len() - 1;
        }

        let mut out = Vec::with_capacity(len);

        out.push(b'[');

        let mut first = true;

        for r in list {
            if first {
                first = false;
            } else {
                out.push(b',');
            }
            r.read(&mut out)?;
        }

        out.push(b']');

        debug_assert_eq!(len, out.len());

        Ok(out)
    }

    /// Update a space, clearing out any expired infos and optionally
    /// adding a new incoming info. If adding a new info, perform all
    /// validation before calling this function.
    pub fn update(
        &self,
        max_entries: usize,
        mut new_info: Option<(crate::ParsedEntry, Option<StoreEntryRef>)>,
    ) {
        // get the current system time
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .expect("InvalidSystemTime")
            .as_micros() as i64;

        // only allow one write at a time
        let write_lock = self.write_lock.lock().unwrap();

        // keep the readable lock time to a minimum
        let list = self.readable.lock().unwrap().clone();

        // if we have a new info to add, pull out the agent key
        // so we can check to replace an existing one while
        // iterating the existing infos
        let replace_agent: Option<ed25519_dalek::VerifyingKey> =
            new_info.as_ref().map(|(p, _)| p.agent);

        // parse the current entries
        let mut list = list
            .into_iter()
            .filter_map(|mut store| {
                let mut parsed = match store.parse() {
                    Ok(p) => p,
                    Err(_) => return None,
                };

                // if this matches an existing info, and the new one is newer
                // use the new one. note, we'll still check for expiration
                // after this step.
                if Some(parsed.agent) == replace_agent {
                    // can unwrap because this can only be true if is_some
                    let (new_p, new_s) = new_info.take().unwrap();
                    if new_p.created_at > parsed.created_at {
                        if new_p.is_tombstone {
                            return None;
                        }

                        parsed = new_p;
                        store = new_s.expect("RequireStoreRef");
                    }
                }

                // check for expiration
                if parsed.expires_at <= now {
                    return None;
                }

                Some((parsed, store))
            })
            .collect::<Vec<_>>();

        // if we still have a new info to add, it must not have matched
        // any existing ones
        if let Some((parsed, store)) = new_info
            && !parsed.is_tombstone
        {
            // if we are full, follow the "default" strategy rule of
            // deleting the half-way info
            if list.len() >= max_entries {
                list.remove(max_entries / 2);
            }

            // push the new info onto the stack
            list.push((parsed, store.expect("RequireStoreRef")));
        }

        // drop the parsed versions
        let list = list.into_iter().map(|e| e.1).collect::<Vec<_>>();

        // update the reader at the end
        *self.readable.lock().unwrap() = list;

        drop(write_lock);
    }
}
