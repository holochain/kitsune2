//! Types dealing with data identity or hashing.

macro_rules! imp_deref {
    ($i:ty, $t:ty) => {
        impl std::ops::Deref for $i {
            type Target = $t;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

macro_rules! imp_from {
    ($a:ty, $b:ty, $i:ident => $e:expr) => {
        impl From<$b> for $a {
            fn from($i: $b) -> Self {
                $e
            }
        }
    };
}

/// Base data identity type meant for newtyping.
/// You probably want [AgentId] or [OpId].
///
/// In Kitsune2 these bytes should ONLY be the actual hash bytes
/// or public key of the identity being tracked, without
/// prefix or suffix.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub bytes::Bytes);

impl serde::Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use base64::prelude::*;
        serializer.serialize_str(&BASE64_URL_SAFE_NO_PAD.encode(&self.0))
    }
}

impl<'de> serde::Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use base64::prelude::*;
        let s: &'de str = serde::Deserialize::deserialize(deserializer)?;
        BASE64_URL_SAFE_NO_PAD
            .decode(s)
            .map(|v| Id(bytes::Bytes::copy_from_slice(&v)))
            .map_err(serde::de::Error::custom)
    }
}

imp_deref!(Id, bytes::Bytes);
imp_from!(Id, bytes::Bytes, b => Id(b));

impl Id {
    /// Get the location u32 based off this Id.
    ///
    /// This is accomplished by directly xor-ing every successive 4 bytes
    /// in the hash. It is okay if the hash len is not a multiple of 4,
    /// it will stop with the ending byte of the hash.
    ///
    /// The remaining 4 bytes are then interpreted as a little-endian u32.
    //
    // Holochain previously would re-hash the hash, and then
    // xor to shrink down to a u32. This extra step is not needed
    // and does not provide any benefit. One extra hash step does
    // not prevent location farming, and if the original hash was
    // distributed well enough, re-hashing it again doesn't improve
    // distribution.
    pub fn loc(&self) -> u32 {
        let mut out = [0_u8; 4];
        let mut i = 0;
        for c in &self.0 {
            out[i] ^= c;
            i += 1;
            if i > 3 {
                i = 0;
            }
        }
        u32::from_le_bytes(out)
    }
}

/// The function signature for Id display overrides.
pub type DisplayCb =
    fn(&bytes::Bytes, &mut std::fmt::Formatter<'_>) -> std::fmt::Result;

/// The default display function encodes the Id as base64.
/// This makes debugging so much easier than rust's default of decimal array.
fn default_display(
    b: &bytes::Bytes,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    use base64::prelude::*;
    f.write_str(&BASE64_URL_SAFE_NO_PAD.encode(b))
}

#[inline(always)]
fn display(
    b: &bytes::Bytes,
    f: &mut std::fmt::Formatter<'_>,
    l: &std::sync::OnceLock<DisplayCb>,
) -> std::fmt::Result {
    l.get_or_init(|| default_display)(b, f)
}

static AGENT_DISP: std::sync::OnceLock<DisplayCb> = std::sync::OnceLock::new();

/// Identifies an agent to be tracked as part of a Kitsune space.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct AgentId(pub Id);

imp_deref!(AgentId, Id);
imp_from!(AgentId, bytes::Bytes, b => AgentId(Id(b)));
imp_from!(AgentId, Id, b => AgentId(b));

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display(&self.0 .0, f, &AGENT_DISP)
    }
}

impl std::fmt::Debug for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display(&self.0 .0, f, &AGENT_DISP)
    }
}

impl AgentId {
    /// Set the display/debug implementation for AgentId for the duration
    /// of this process. Note, if anything was printed earlier, the
    /// default impl will have been set and cannot be changed.
    /// Returns false if the default was unable to be set.
    pub fn set_global_display_callback(cb: DisplayCb) -> bool {
        AGENT_DISP.set(cb).is_ok()
    }
}

static SPACE_DISP: std::sync::OnceLock<DisplayCb> = std::sync::OnceLock::new();

/// Identifies a space to be tracked by Kitsune.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct SpaceId(pub Id);

imp_deref!(SpaceId, Id);
imp_from!(SpaceId, bytes::Bytes, b => SpaceId(Id(b)));
imp_from!(SpaceId, Id, b => SpaceId(b));

impl std::fmt::Display for SpaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display(&self.0 .0, f, &SPACE_DISP)
    }
}

impl std::fmt::Debug for SpaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display(&self.0 .0, f, &SPACE_DISP)
    }
}

impl SpaceId {
    /// Set the display/debug implementation for SpaceId for the duration
    /// of this process. Note, if anything was printed earlier, the
    /// default impl will have been set and cannot be changed.
    /// Returns false if the default was unable to be set.
    pub fn set_global_display_callback(cb: DisplayCb) -> bool {
        SPACE_DISP.set(cb).is_ok()
    }
}

static OP_DISP: std::sync::OnceLock<DisplayCb> = std::sync::OnceLock::new();

/// Identifies an op to be tracked by Kitsune.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct OpId(pub Id);

imp_deref!(OpId, Id);
imp_from!(OpId, bytes::Bytes, b => OpId(Id(b)));
imp_from!(OpId, Id, b => OpId(b));

impl std::fmt::Display for OpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display(&self.0 .0, f, &OP_DISP)
    }
}

impl std::fmt::Debug for OpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display(&self.0 .0, f, &OP_DISP)
    }
}

impl OpId {
    /// Set the display/debug implementation for OpId for the duration
    /// of this process. Note, if anything was printed earlier, the
    /// default impl will have been set and cannot be changed.
    /// Returns false if the default was unable to be set.
    pub fn set_global_display_callback(cb: DisplayCb) -> bool {
        OP_DISP.set(cb).is_ok()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn loc_zero_is_zero() {
        assert_eq!(0, Id(bytes::Bytes::from_static(b"")).loc());
    }

    #[test]
    fn loc_u32_equality() {
        for u in [0, 42, 999, u32::MAX / 13, u32::MAX / 4, u32::MAX] {
            assert_eq!(
                u,
                Id(bytes::Bytes::copy_from_slice(&u.to_le_bytes())).loc()
            );
        }
    }

    #[test]
    fn loc_fixtures() {
        const F: &[(&[u8], u32)] = &[
            (b"hello", 1819043079),
            (b"1", 49),
            (b"asntoheunatoheuntahoeusth", 454101873),
            (&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff], 4294901760),
            (&[1, 2, 3, 230, 44, 77, 99, 82], 3026210605),
            (&[42, 0, 0, 0, 99, 0, 0, 0], 73),
        ];

        for (b, res) in F.iter() {
            assert_eq!(*res, Id(bytes::Bytes::from_static(b)).loc());
        }
    }

    #[test]
    fn id_serde_fixtures() {
        const F: &[(&[u8], &str)] = &[
            (b"test-hash-1", "\"dGVzdC1oYXNoLTE\""),
            (b"s", "\"cw\""),
            (&[255, 255, 255, 255, 255, 255, 255], "\"_________w\""),
            (b"here is a very long string here is a very long string here is a very long string here is a very long string here is a very long string here is a very long string here is a very long string here is a very long string ", "\"aGVyZSBpcyBhIHZlcnkgbG9uZyBzdHJpbmcgaGVyZSBpcyBhIHZlcnkgbG9uZyBzdHJpbmcgaGVyZSBpcyBhIHZlcnkgbG9uZyBzdHJpbmcgaGVyZSBpcyBhIHZlcnkgbG9uZyBzdHJpbmcgaGVyZSBpcyBhIHZlcnkgbG9uZyBzdHJpbmcgaGVyZSBpcyBhIHZlcnkgbG9uZyBzdHJpbmcgaGVyZSBpcyBhIHZlcnkgbG9uZyBzdHJpbmcgaGVyZSBpcyBhIHZlcnkgbG9uZyBzdHJpbmcg\""),
        ];

        for (d, e) in F.iter() {
            let r = serde_json::to_string(&Id(bytes::Bytes::from_static(d)))
                .unwrap();
            assert_eq!(e, &r);
            let r: AgentId = serde_json::from_str(e).unwrap();
            assert_eq!(d, &r.0 .0);
        }
    }
}
