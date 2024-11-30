use std::{collections::HashSet, sync::Arc};

use kitsune2_api::{
    fetch::{
        DynFetchQueue, DynFetchQueueFactory, FetchQueue, FetchQueueFactory,
    },
    AgentId, BoxFut, K2Result, OpId,
};

#[derive(Debug)]
struct Q {
    ops: Vec<OpId>,
    sources: HashSet<AgentId>,
}

impl Q {
    fn new() -> Self {
        Self {
            ops: Vec::new(),
            sources: HashSet::new(),
        }
    }
}

impl FetchQueue for Q {
    fn add_ops(&mut self, op_list: Vec<OpId>, source: AgentId) {
        op_list.into_iter().for_each(|op| {
            if !self.ops.contains(&op) {
                self.ops.push(op);
            }
        });
        self.sources.insert(source);
    }
}

#[derive(Debug)]
pub struct QFactory {}

impl QFactory {
    fn create() -> DynFetchQueueFactory {
        Arc::new(Self {})
    }
}

impl FetchQueueFactory for QFactory {
    fn create(&self) -> BoxFut<'static, K2Result<DynFetchQueue>> {
        Box::pin(async move { Arc::new(Q::new()) })
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use kitsune2_api::{id::Id, AgentId, OpId};
    use rand::Rng;

    use crate::fetch_queue::FetchQueue;

    #[test]
    fn add_ops() {
        let mut fetch_queue = FetchQueue::new();
        let op_list_1 = create_op_list(1);
        let source = random_agent_id();
        fetch_queue.add_ops(op_list_1.clone(), source);

        assert_eq!(
            fetch_queue.get_ops_to_fetch(),
            op_list_1.clone().into_iter().rev().collect::<Vec<_>>()
        );

        let op_list_2 = create_op_list(2);
        let source = random_agent_id();
        fetch_queue.add_ops(op_list_2.clone(), source);

        assert_eq!(
            fetch_queue.get_ops_to_fetch(),
            op_list_1
                .into_iter()
                .chain(op_list_2.into_iter())
                .rev()
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
