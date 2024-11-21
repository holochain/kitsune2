use crate::Timestamp;
use kitsune2_api::OpId;

#[derive(Debug)]
pub enum GossipMessage {
    PeerInitiate(PeerHello),
    PeerAccept {
        peer_hello: PeerHello,
        new_op_hashes: Vec<OpId>,
        bookmark_timestamp: Timestamp,
    },
    PeerCompleteAfterAccept {
        new_op_hashes: Vec<OpId>,
        bookmark_timestamp: Timestamp,
    },
}

/// The first message sent by each peer when starting a gossip round.
///
/// The initiating gathers their state and sends it as the initiation message. The accepting peer
/// does the same and response in the accept message.
#[derive(Debug)]
pub struct PeerHello {
    /// The chunks states for the gossip overlap.
    pub chunk_states: Vec<ChunkState>,

    /// Request new ops since this timestamp.
    pub new_ops_since: Timestamp,

    /// The maximum number of bytes of new ops to respond with.
    pub new_ops_max_bytes: u32,
}

#[derive(Debug)]
pub struct ChunkState {
    pub chunk_id: u32,
    pub slice_state: SliceState,
}

#[derive(Debug)]
pub struct SliceState {
    pub slice_count: u64,
    pub combined_hash: Vec<u8>,
}
