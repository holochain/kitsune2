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

/// Base data identity type meant for newtyping.
/// You probably want [AgentId] or [OpId].
///
/// In Kitsune2 these bytes should ONLY be the actual hash bytes
/// or public key of the identity being tracked, without
/// prefix or suffix.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub bytes::Bytes);

imp_deref!(Id, bytes::Bytes);

impl Id {
    /// Get the location u32 based off this Id.
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
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentId(pub Id);

imp_deref!(AgentId, Id);

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
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpaceId(pub Id);

imp_deref!(SpaceId, Id);

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
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpId(pub Id);

imp_deref!(OpId, Id);

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
