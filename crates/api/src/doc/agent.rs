//! # Agent metadata documentation.
//!
//! [AgentInfo] and the wrapping [AgentInfoSigned] define a pattern for
//! cryptographically verifiable declarations of network reachability.
//!
//! To facilitate ease of debugging (See our unit tests in this module!),
//! The canonical encoding for this info is JSON.
//!
//! #### Json Schemas
//!
//! ```json
//! {
//!   "title": "AgentInfoSigned",
//!   "type": "object",
//!   "properties": {
//!     "agentInfo": { "type": "string", "required": true, "description": "json AgentInfo" },
//!     "signature": { "type": "string", "required": true, "description": "base64" }
//!   }
//! }
//! ```
//!
//! ```json
//! {
//!   "title": "AgentInfo",
//!   "type": "object",
//!   "properties": {
//!     "agent": { "type": "string", "required": true, "description": "base64" },
//!     "space": { "type": "string", "required": true, "description": "base64" },
//!     "createdAt": {
//!         "type": "string",
//!         "required": true,
//!         "description": "i64 micros since unix epoch"
//!     },
//!     "expiresAt": {
//!         "type": "string",
//!         "required": true,
//!         "description": "i64 micros since unix epoch",
//!     },
//!     "isTombstone": { "type": "boolean", "required": true },
//!     "url": { "type": "string", "description": "optional" },
//!     "storageArc": {
//!       "type": "array",
//!       "description": "optional",
//!       "items": [
//!         {
//!             "type": "number",
//!             "required": true,
//!             "description": "u32 arc start loc"
//!         },
//!         {
//!             "type": "number",
//!             "required": true,
//!             "description": "u32 arc end loc"
//!         }
//!       ]
//!     }
//!   }
//! }
//! ```
//!
//! #### Cryptography
//!
//! This module and its data structures are designed to be agnostic to
//! cryptography. It exposes the [Signer] and [Verifier] traits to allow
//! implementors to choose the algorithm to be used.
//!
//! The underlying data structures, however, cannot be quite so agnostic.
//!
//! By convention, absent other indications, the [crate::AgentInfo::agent] property
//! will be an ed25519 public key, and the [crate::AgentInfoSigned::get_signature] will
//! be an ed25519 signature.
//!
//! Future versions of this library may look for an optional "alg" property
//! on the [crate::AgentInfo] type before falling back to this usage of ed25519.
//! These other algorithms may treat the [crate::AgentInfo::agent] property as a
//! hash of the public key instead of the public key itself, and find the
//! public key instead on an "algPubKey" property. (Some post-quantum
//! algorithms have ridiculously long key material.)
//!
//! [AgentInfo]: crate::AgentInfo
//! [AgentInfoSigned]: crate::AgentInfoSigned
//! [Signer]: crate::Signer
//! [Verifier]: crate::Verifier
