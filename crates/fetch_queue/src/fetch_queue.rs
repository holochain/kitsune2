use std::collections::HashSet;

use kitsune2_api::{fetch::FetchQueueT, AgentId, OpId};
use rand::seq::IteratorRandom;

#[derive(Debug)]
pub struct FetchQueue {
    ops: Vec<OpId>,
    sources: HashSet<AgentId>,
}

impl FetchQueueT for FetchQueue {
    fn add_ops(&mut self, op_list: Vec<OpId>, source: AgentId) {
        op_list.into_iter().for_each(|op| {
            if !self.ops.contains(&op) {
                self.ops.push(op);
            }
        });
        self.sources.insert(source);
    }

    fn get_ops_to_fetch(&self) -> Vec<OpId> {
        self.ops.clone()
    }

    fn get_random_source(&self) -> Option<AgentId> {
        if self.sources.is_empty() {
            None
        } else {
            let mut rng = rand::thread_rng();
            self.sources
                .iter()
                .choose(&mut rng)
                .map(|agent_id| agent_id.clone())
        }
    }
}

impl FetchQueue {
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            sources: HashSet::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use kitsune2_api::{id::Id, AgentId, OpId};
    use rand::Rng;

    use crate::fetch_queue::{FetchQueue, FetchQueueT};

    #[test]
    fn add_ops() {
        let mut fetch_queue = FetchQueue::new();
        let op_list_1 = create_op_list(1);
        let source = random_agent_id();
        fetch_queue.add_ops(op_list_1.clone(), source);

        assert_eq!(fetch_queue.get_ops_to_fetch(), op_list_1);

        let op_list_2 = create_op_list(2);
        let source = random_agent_id();
        fetch_queue.add_ops(op_list_2.clone(), source);

        assert_eq!(
            fetch_queue.get_ops_to_fetch(),
            op_list_1
                .into_iter()
                .chain(op_list_2.into_iter())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn get_source() {
        let mut fetch_queue = FetchQueue::new();
        assert_eq!(fetch_queue.get_random_source(), None);

        let op_list = create_op_list(1);
        let source_1 = random_agent_id();
        fetch_queue.add_ops(op_list.clone(), source_1.clone());

        assert_eq!(fetch_queue.get_random_source(), Some(source_1.clone()));

        let op_list = create_op_list(2);
        let source_2 = random_agent_id();
        fetch_queue.add_ops(op_list, source_2.clone());

        assert!([source_1.clone(), source_2.clone()]
            .contains(&fetch_queue.get_random_source().unwrap()));
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

    fn create_op_list(num_ops: u16) -> Vec<OpId> {
        let mut ops = Vec::new();
        for _ in 0..num_ops {
            let op = random_op_id();
            ops.push(op.clone());
        }
        ops
    }
}
