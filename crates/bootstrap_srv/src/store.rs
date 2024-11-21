//! This is a virtual-memory inspired tempfile store.
//!
//! ### Rationale
//!
//! - We don't need to persist anything in the store beyond a single
//!   process invocation, because the infos are going to expire after
//!   a matter of minutes anyways, and peers will continue to re-publish them.
//! - We would like a server to be able to store more spaces and infos
//!   than would reasonably fit in RAM.
//!
//! ### Implementation
//!
//! - Have a pool of tempfiles that can grow to match the worker thread
//!   count if needed.
//! - Have the ability to Clone (reopen) these tempfile handles.
//! - Designate one file handle only per pool entry as writable.
//! - All other handles will be readonly.
//! - After writing an entry, return a reference with a readonly file handle
//!   to the tempfile that was written to, and an offset/length to
//!   allow readback of exactly the bytes written for that entry.
//! - Once a tempfile reaches a certain size, drop the write handle and
//!   open a new tempfile for writing.
//! - The older read handles will persist the existence of the older tempfiles
//!   until the last read reference is dropped, at which point the tempfile
//!   will be cleaned up by the os.
//! - We can trust these files will cycle through at a rate similar to the
//!   max expiration time on the infos they contain (30 minutes).

use std::sync::{Arc, Mutex};

/// How large we should allow individual tempfiles to grow.
/// This is a balance between using up too much disk space
/// and having too many file handles open.
///
/// Start with 10MiB?
const MAX_PER_TEMPFILE: u64 = 1024 * 1024 * 10;

struct Writer {
    writer: std::fs::File,
    read_clone: Arc<tempfile::NamedTempFile>,
}

type WriterPool = Arc<Mutex<std::collections::VecDeque<Writer>>>;

/// A reference to previously written data.
#[derive(Clone)]
pub struct StoreEntryRef {
    read_clone: Arc<tempfile::NamedTempFile>,
    offset: u64,
    length: usize,
}

impl StoreEntryRef {
    /// Get the length of data to be read.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.length
    }

    /// Read the content of this entry. This will extend the provided
    /// buffer with the bytes read from the store.
    pub fn read(&self, buf: &mut Vec<u8>) -> std::io::Result<()> {
        use std::io::{Read, Seek};
        let mut reader = self.read_clone.reopen()?;
        reader.seek(std::io::SeekFrom::Start(self.offset))?;

        buf.reserve(self.length);

        unsafe {
            let offset = buf.len();
            buf.set_len(offset + self.length);

            if let Err(err) =
                reader.read_exact(&mut buf[offset..offset + self.length])
            {
                // On read error, undo the set_len.
                buf.set_len(offset);

                return Err(err);
            }
        }

        Ok(())
    }

    /// Parse this entry.
    pub fn parse(&self) -> std::io::Result<crate::ParsedEntry> {
        let mut tmp = Vec::with_capacity(self.length);
        self.read(&mut tmp)?;
        crate::ParsedEntry::from_slice(&tmp)
    }
}

/// Tempfile-based virtual memory solution.
pub struct Store {
    writer_pool: WriterPool,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            writer_pool: Arc::new(
                Mutex::new(std::collections::VecDeque::new()),
            ),
        }
    }
}

impl Store {
    /// Write an entry to the virtual memory store, getting back
    /// a reference that will allow future reading.
    pub fn write(&self, content: &[u8]) -> std::io::Result<StoreEntryRef> {
        use std::io::{Seek, Write};
        let mut writer = {
            match self.writer_pool.lock().unwrap().pop_front() {
                Some(writer) => writer,
                None => {
                    let read_clone = Arc::new(tempfile::NamedTempFile::new()?);
                    let writer = read_clone.reopen()?;
                    Writer { writer, read_clone }
                }
            }
        };
        let read_clone = writer.read_clone.clone();
        let offset = writer.writer.stream_position()?;
        let length = content.len();

        writer.writer.write_all(content)?;
        writer.writer.sync_data()?;

        if offset + (length as u64) < MAX_PER_TEMPFILE {
            self.writer_pool.lock().unwrap().push_back(writer);
        }

        Ok(StoreEntryRef {
            read_clone,
            offset,
            length,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn happy_sanity() {
        let s = Store::default();

        let hello = s.write(b"Hello ").unwrap();
        let world = s.write(b"world!").unwrap();

        let mut buf = Vec::new();
        hello.read(&mut buf).unwrap();
        world.read(&mut buf).unwrap();

        assert_eq!(b"Hello world!", buf.as_slice());
    }

    #[test]
    fn happy_multi_thread_sanity() {
        const COUNT: usize = 10;
        let mut all = Vec::with_capacity(COUNT);

        let s = Arc::new(Store::default());
        let b = Arc::new(std::sync::Barrier::new(COUNT));

        let (send, recv) = std::sync::mpsc::channel();

        for i in 0..COUNT {
            let send = send.clone();
            let s = s.clone();
            let b = b.clone();

            all.push(std::thread::spawn(move || {
                b.wait();
                let wrote = format!("index:{i}");
                let r = s.write(wrote.as_bytes()).unwrap();
                send.send((wrote, r)).unwrap();
            }));
        }

        for _ in 0..COUNT {
            let (wrote, r) = recv.recv().unwrap();
            let mut read = Vec::new();
            r.read(&mut read).unwrap();
            assert_eq!(wrote.as_bytes(), read);
        }
    }
}
