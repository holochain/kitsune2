//! Url-related types.

use crate::*;

// We're using bytes::Bytes as the storage type for urls instead of String,
// even though it adds a little complexity overhead to the accessor functions
// here, because Bytes are more cheaply clone-able, and we need it to be bytes
// for the protobuf wire message types.

/// A validated Kitsune2 Url.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Url(bytes::Bytes);

impl serde::Serialize for Url {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Url {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct V;

        impl serde::de::Visitor<'_> for V {
            type Value = bytes::Bytes;

            fn expecting(
                &self,
                f: &mut std::fmt::Formatter,
            ) -> std::fmt::Result {
                f.write_str("a valid Kitsune2 Url")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(bytes::Bytes::copy_from_slice(v))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(bytes::Bytes::copy_from_slice(v.as_bytes()))
            }
        }

        let b = deserializer.deserialize_bytes(V)?;

        Url::new(b).map_err(serde::de::Error::custom)
    }
}

impl From<Url> for bytes::Bytes {
    fn from(u: Url) -> Self {
        u.0
    }
}

impl From<&Url> for bytes::Bytes {
    fn from(u: &Url) -> Self {
        u.0.clone()
    }
}

impl AsRef<str> for Url {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::convert::TryFrom<bytes::Bytes> for Url {
    type Error = K2Error;

    fn try_from(b: bytes::Bytes) -> Result<Self, Self::Error> {
        Self::new(b)
    }
}

impl std::fmt::Display for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Debug for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl std::str::FromStr for Url {
    type Err = K2Error;

    fn from_str(src: &str) -> Result<Self, Self::Err> {
        Self::from_str(src)
    }
}

impl Url {
    /// Construct a new validated Kitsune2 Url.
    pub fn new(src: bytes::Bytes) -> K2Result<Self> {
        let str_src = std::str::from_utf8(&src).map_err(|err| {
            K2Error::other_src("Kitsne2 Url is not valid utf8", err)
        })?;

        let parsed = ::url::Url::parse(str_src).map_err(|err| {
            K2Error::other_src("Could not parse as Kitsune2 Url", err)
        })?;

        let scheme = match parsed.scheme() {
            scheme @ "ws" | scheme @ "wss" => scheme,
            oth => {
                return Err(K2Error::other(format!(
                    "Invalid Kitsune2 Url Scheme: {oth}",
                )));
            }
        };

        let host = match parsed.host_str() {
            Some(host) => host,
            None => {
                return Err(K2Error::other(
                    "Invalid Kitsune2 Url, Missing Host",
                ));
            }
        };

        let port = match parsed.port_or_known_default() {
            Some(port) => port,
            None => {
                return Err(K2Error::other(
                    "Invalid Kitsune2 Url, Explicit Port Required",
                ));
            }
        };

        let path = parsed.path();

        if path.split('/').count() != 2 {
            return Err(K2Error::other(
                "Invalid Kitsune2 Url, path must contain exactly 1 slash",
            ));
        }

        let canonical = if path == "/" {
            format!("{scheme}://{host}:{port}")
        } else {
            format!("{scheme}://{host}:{port}{path}")
        };

        if str_src != canonical.as_str() {
            return Err(K2Error::other(format!(
                "Invalid Kitsune2 Url, Non-Canonical. Expected: {canonical}. Got: {str_src}",
            )));
        }

        Ok(Self(src))
    }

    /// Construct a new validated Kitsune2 Url from a str.
    // We *do* also implement the trait. But it's not as usable,
    // so implement a better local version as well.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str<S: AsRef<str>>(src: S) -> K2Result<Self> {
        Self::new(bytes::Bytes::copy_from_slice(src.as_ref().as_bytes()))
    }

    /// Get this url as a str.
    pub fn as_str(&self) -> &str {
        // we've already checked it is valid utf8 in the constructor.
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }

    /// Returns true if the protocol scheme is `wss`.
    pub fn uses_tls(&self) -> bool {
        &self.0[..3] == b"wss"
    }

    /// Returns true if this is a peer url. Otherwise, this is a server url.
    pub fn is_peer(&self) -> bool {
        self.peer_id().is_some()
    }

    /// Returns the peer id if this is a peer url.
    pub fn peer_id(&self) -> Option<&str> {
        match self.as_str().split_once("://") {
            None => None,
            Some((_, r)) => match r.rsplit_once('/') {
                None => None,
                Some((_, r)) => {
                    if r.is_empty() {
                        None
                    } else {
                        Some(r)
                    }
                }
            },
        }
    }

    /// Returns the host:port to use for connecting to this url.
    pub fn addr(&self) -> &str {
        // unwraps in here because this has all been validated by constructor
        let addr = self.as_str().split_once("://").unwrap().1;
        match addr.split_once('/') {
            None => addr,
            Some((addr, _)) => addr,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn happy_serialize() {
        const URL: &str = "wss://test.com:443";
        let u = Url::from_str(URL).unwrap();
        let e = serde_json::to_string(&u).unwrap();
        assert_eq!(format!("\"{URL}\""), e);
        let d: Url = serde_json::from_str(&e).unwrap();
        assert_eq!(d, u);
    }

    #[test]
    fn fixture_parse() {
        const F: &[(&str, Option<&str>, bool, &str)] = &[
            ("ws://a.b:80", None, false, "a.b:80"),
            ("ws://1.1.1.1:80", None, false, "1.1.1.1:80"),
            ("ws://[::1]:80", None, false, "[::1]:80"),
            ("wss://a.b:443", None, true, "a.b:443"),
            ("ws://a.b:999", None, false, "a.b:999"),
            ("ws://a.b:80/foo", Some("foo"), false, "a.b:80"),
            ("wss://a.b:443/foo", Some("foo"), true, "a.b:443"),
            ("ws://a.b:999/foo", Some("foo"), false, "a.b:999"),
        ];

        for (s, id, tls, addr) in F.iter() {
            let u = Url::from_str(s).unwrap();
            assert_eq!(s, &u.as_str());
            assert_eq!(id, &u.peer_id());
            assert_eq!(tls, &u.uses_tls());
            assert_eq!(addr, &u.addr());
        }
    }

    #[test]
    fn fixture_no_parse() {
        const F: &[&str] = &[
            "ws://a.b",
            "wss://a.b",
            "w://a.b:80",
            "ws://a.b:80/",
            "ws://a.b:80/foo/bar",
        ];

        for s in F.iter() {
            assert!(Url::from_str(s).is_err());
        }
    }
}
