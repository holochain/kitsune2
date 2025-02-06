#![deny(missing_docs)]
//! A distributed hash table (DHT) implementation for use in Kitsune2.
//!
//! The DHT model partitions data by location and time. The location is derived from the hash of the data, and is a 32-bit
//! number. The time is a 64-bit number, representing a number of microseconds since the Unix epoch.
//!
//! The model first organises by location, then within a set of locations, it organises by time. The way that the model is
//! partitioned is designed to permit computing a diff against another DHT model. This is the primary purpose of this crate,
//! to enable efficient syncing of DHT models between agents.
//!
//! You can think of the DHT as a graph, where the vertical axis represents location and the horizontal axis represents time.
//! Every piece of data can be represented as a point on this graph:
//!
//! <details style="cursor:pointer">
//!     <summary>DHT model (graph representation)</summary>
#![doc = include_str!("../art/dht-graph.svg")]
//! </details>
//!
//! Notice that the partitioning introduces some new terminology:
//! - **Sector**: The location space is split into equally-sized chunks called sectors. A sector spans the entire time axis.
//! - **Time slice**: The horizontal axis is split into time slices. These come in two flavours, full and partial. The light
//!   blue slices are full, and the dark blue slices are partial. The full time slices are fixed-size, while the partial
//!   time slices start at half the size of a full time slice and halve in size towards the current time.
//! - **Disc**: The disc is the light blue area of the graph. It is defined by the set of full time slices and the full set
//!   of sectors.
//! - **Ring**: A ring is one vertical section in the dark blue area of the graph. It is defined by a partial time slice and
//!   the full set of sectors.
//!
//! The reason for the geometric naming is that the locations are thought of as a circle, with the highest location being
//! adjacent to the lowest location. This is not a perfect analogy, but it is useful to have terminology that describes
//! areas of the graph in a consistent way. See the following diagram:
//!
//! <details style="cursor:pointer">
//!     <summary>DHT model (circle representation)</summary>
#![doc = include_str!("../art/dht-circle.svg")]
//! </details>
//!
//! With time starting at the center of the circle and moving outwards, and location segments delimited by straight lines
//! radiating from the center. Then we can see the definition of the terms above more clearly. The disc is the light blue
//! area, which is a disc in the geometric sense, bounded by the end of the last full time slice. The rings are the dark
//! blue area, delimited by the end of the last partial time slice and the current time. The time slices are the concentric
//! circles, delimited by white lines, with the light blue ones being full and the dark blue ones being partial.
//!
//! Each contained area within this shape can be represented by a single "combined hash". A "combined hash" is simply a
//! combination of all the hashes of data within that area. A "combined hash" is generally pre-computed or updated
//! incrementally, as opposed to a "top hash". The term "top hash" is used to refer to a combination of "combined hash"es.
//! For example, computing a "top hash" of the disc would involve taking all the combined hashes for full time slices in
//! each sector and combining them, then combining all of those across the sectors.
//!
//! There are detailed explanations of the pieces of the model in each module. To aid with navigation, here is a brief
//! overview of the modules:
//! - The model starts in the `time` module. This module defines time slices, and handles combining hashes
//!   within time slices. It can also combine hashes to yield top hashes.
//! - The next step in the model is the `hash` module. This module defines the partition over location, and
//!   uses the `time` module for its inner state.
//! - The supporting module `arc_set` defines a set of agent arcs. This module provides a simple
//!   representation of the overlap between the storage [DhtArc](kitsune2_api::DhtArc)s' of two agents.
//! - The top level module is the `dht` module. This module uses the hash module as its inner state, and adds
//!   the logic for computing a diff between two DHT models. This is a multistep process, and is designed to balance the
//!   amount of data that must be sent, with the number of round-trips required to sync the models.

mod arc_set;
mod constant;
mod dht;
mod hash;
mod time;

mod combine;
#[cfg(test)]
mod test;

pub use arc_set::*;
pub use constant::*;
pub use dht::{snapshot::*, *};
pub use hash::*;
pub use time::*;
