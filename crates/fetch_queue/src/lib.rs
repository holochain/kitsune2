//! The fetch queue is a helper module to fetch a set of kitsune ops from a peer.
//!
//! When a peer initiates a gossip session with another peer, it compares the ops
//! held by both and determines which ops it is missing. These ops must then be
//! fetched using the fetch queue.
//!
//! Gossip sessions are performed one after the other. Every session can have a set
//! of missing ops as an outcome. Ops can be held by multiple peers in the network,
//! so the fetch queue must be capable of keeping track of which ops can be fetched
//! from which peers.
//!
//! It must be possible to discover sources of an op.

use std::collections::{hash_map::Entry, HashMap, HashSet};

use kitsune2_api::{AgentId, OpId};

trait FetchQueueT {
    fn new() -> Self;
    fn add_ops(&mut self, op_list: OpList);
}

#[derive(Debug)]
struct OpList {
    ops: HashSet<OpId>,
    source: AgentId,
}

#[derive(Debug)]
struct FetchQueue {
    ops: HashMap<OpId, HashSet<AgentId>>,
}

impl FetchQueueT for FetchQueue {
    fn new() -> Self {
        Self {
            ops: HashMap::new(),
        }
    }

    fn add_ops(&mut self, ops: OpList) {
        ops.ops
            .iter()
            .for_each(|op| match self.ops.entry(op.clone()) {
                Entry::Occupied(mut o) => {
                    o.get_mut().insert(ops.source.clone());
                }
                Entry::Vacant(v) => {
                    let mut sources = HashSet::new();
                    sources.insert(ops.source.clone());
                    v.insert(sources);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bytes::Bytes;
    use kitsune2_api::{id::Id, AgentId, OpId};
    use rand::{random, Rng, RngCore};

    use crate::{FetchQueue, FetchQueueT, OpList};

    #[test]
    fn add_ops() {
        let mut fetch_queue = FetchQueue::new();

        let mut ops = HashSet::new();
        let op_1 = random_op_id();
        ops.insert(op_1.clone());
        let source_1 = random_agent_id();
        let op_list = OpList {
            ops,
            source: source_1.clone(),
        };
        fetch_queue.add_ops(op_list);

        let mut ops = HashSet::new();
        let op_2 = random_op_id();
        let op_3 = random_op_id();
        ops.insert(op_2.clone());
        ops.insert(op_3.clone());
        let source_2 = random_agent_id();
        let op_list = OpList {
            ops,
            source: source_2.clone(),
        };
        fetch_queue.add_ops(op_list);

        assert!(fetch_queue.ops.get(&op_1).unwrap().contains(&source_1));
        assert!(fetch_queue.ops.get(&op_2).unwrap().contains(&source_2));
        assert!(fetch_queue.ops.get(&op_3).unwrap().contains(&source_2));
        assert!(!fetch_queue.ops.get(&op_1).unwrap().contains(&source_2));
        assert!(!fetch_queue.ops.get(&op_2).unwrap().contains(&source_1));
        assert!(!fetch_queue.ops.get(&op_3).unwrap().contains(&source_1));
    }

    fn random_id() -> Id {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 32];
        rng.fill(&mut bytes);
        let bytes = Bytes::from(bytes.to_vec());
        Id(bytes)
    }

    fn random_op_id() -> OpId {
        OpId(random_id())
    }

    fn random_agent_id() -> AgentId {
        AgentId(random_id())
    }
}
