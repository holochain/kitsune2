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
pub struct StoreEntryRef {
    read_clone: Arc<tempfile::NamedTempFile>,
    offset: u64,
    length: usize,
}

impl StoreEntryRef {
    /// Read the content of this entry.
    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek};
        let mut reader = self.read_clone.reopen()?;
        reader.seek(std::io::SeekFrom::Start(self.offset))?;

        let mut out = Vec::with_capacity(self.length);

        unsafe {
            // We won't use this buffer unless it is fully written within this
            // block, so it is safe to have it contain uninitialized bytes
            // here for one line.
            out.set_len(self.length);
            reader.read_exact(&mut out)?;
        }

        Ok(out)
    }
}

/// Tempfile-based virtual memory solution.
pub struct Store {
    writer_pool: WriterPool,
}

impl Store {
    /// Construct a new virtual memory store.
    pub fn new() -> Self {
        Self {
            writer_pool: Arc::new(
                Mutex::new(std::collections::VecDeque::new()),
            ),
        }
    }

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
        let s = Store::new();

        let hello = s.write(b"hello").unwrap();
        let world = s.write(b"world").unwrap();

        assert_eq!(b"hello", hello.read().unwrap().as_slice());
        assert_eq!(b"world", world.read().unwrap().as_slice());
    }

    #[test]
    fn happy_multi_thread_sanity() {
        const COUNT: usize = 10;
        let mut all = Vec::with_capacity(COUNT);

        let s = Arc::new(Store::new());
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
            let read = r.read().unwrap();
            assert_eq!(wrote.as_bytes(), read);
        }
    }
}
