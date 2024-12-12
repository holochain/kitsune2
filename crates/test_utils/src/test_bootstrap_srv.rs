//! Utilites for running and connecting to a test bootstrap server.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use kitsune2_bootstrap_srv::*;

/// A reference to a test bootstrap server.
/// When the last reference to this is dropped, the server will shut down.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TestBootstrapSrv(Arc<str>);

impl Drop for TestBootstrapSrv {
    fn drop(&mut self) {
        get_stat().handle_drop(self.0.clone());
    }
}

impl TestBootstrapSrv {
    /// Construct a new TestBootstrapSrv instance by passing a test uri.
    /// This uri should take the form: `test:<my-unique-string-here>`.
    /// Any TestBootstrapSrv instances with the same uri will be connected
    /// to the same BootstrapSrv instance.
    pub fn new<R: AsRef<str>>(test_uri: R) -> Arc<Self> {
        Arc::new_cyclic(|weak| {
            let test_uri: Arc<str> =
                test_uri.as_ref().to_string().into_boxed_str().into();
            assert!(test_uri.starts_with("test:"));
            get_stat().create_or_append(test_uri.clone(), weak.clone());
            Self(test_uri)
        })
    }

    /// Get the server address for connecting to this bootstrap server.
    pub fn server_address(&self) -> String {
        get_stat().server_address(&self.0)
    }
}

type StatEntry = (BootstrapSrv, Vec<Weak<TestBootstrapSrv>>);

/// A global static type for storing test instances.
#[derive(Default)]
struct Stat(Mutex<HashMap<Arc<str>, StatEntry>>);

impl Stat {
    /// Create a new bootstrap server instance if one doesn't already exist.
    /// Otherwise add our keep-alive reference to the existing one.
    pub fn create_or_append(
        &self,
        key: Arc<str>,
        weak: Weak<TestBootstrapSrv>,
    ) {
        use std::collections::hash_map::Entry;

        let mut lock = self.0.lock().unwrap();

        match lock.entry(key) {
            Entry::Occupied(mut e) => {
                e.get_mut().1.push(weak);
            }
            Entry::Vacant(e) => {
                let config = Config::testing();
                let srv =
                    futures::executor::block_on(tokio::task::spawn_blocking(
                        move || BootstrapSrv::new(config).unwrap(),
                    ))
                    .unwrap();
                e.insert((srv, vec![weak]));
            }
        }
    }

    /// Drop the server instance if there are no longer any keep-alive refs.
    pub fn handle_drop(&self, key: Arc<str>) {
        let mut do_not_drop_while_mutex_locked: Option<BootstrapSrv> = None;

        {
            let mut lock = self.0.lock().unwrap();
            if let Some((srv, mut weak)) = lock.remove(&key) {
                // if there are still any strong references, we don't actually
                // want to delete it.
                weak.retain(|weak| weak.strong_count() > 0);
                if !weak.is_empty() {
                    lock.insert(key, (srv, weak));
                } else {
                    do_not_drop_while_mutex_locked = Some(srv);
                }
            }
        }

        drop(do_not_drop_while_mutex_locked)
    }

    /// Get the address for connecting to this bootstrap server.
    pub fn server_address(&self, key: &str) -> String {
        format!(
            "http://{}",
            self.0.lock().unwrap().get(key).unwrap().0.listen_addrs()[0]
        )
    }
}

/// The static global map of testing bootstrap server instances.
static STAT: OnceLock<Stat> = OnceLock::new();
fn get_stat() -> &'static Stat {
    STAT.get_or_init(Stat::default)
}
