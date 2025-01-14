use crate::protocol::{
    serialize_gossip_message, K2GossipAcceptMessage, K2GossipInitiateMessage,
    K2GossipMessage,
};
use kitsune2_api::agent::LocalAgent;
use kitsune2_api::space::DynSpace;
use kitsune2_api::{AgentId, K2Error, K2Result, Url};
use kitsune2_dht::ArcSet;
use std::fmt::Debug;
use tokio::sync::mpsc::Sender;

pub(crate) struct GossipResponse(pub(crate) bytes::Bytes, pub(crate) Url);

pub(crate) async fn local_agent_state(
    space: DynSpace,
) -> K2Result<(Vec<AgentId>, ArcSet)> {
    let local_agents = space.get_local_agents().await?;
    let (send_agents, our_arcs) = local_agents
        .iter()
        .map(|a| (a.agent().clone(), a.get_tgt_storage_arc()))
        .collect::<(Vec<_>, Vec<_>)>();

    println!("Our arcs: {:?}", our_arcs);

    let our_arc_set = ArcSet::new(our_arcs)?;

    Ok((send_agents, our_arc_set))
}

pub(crate) fn send_gossip_message(
    tx: &Sender<GossipResponse>,
    target_url: Url,
    msg: impl Into<K2GossipMessage> + Debug,
) -> K2Result<()> {
    tracing::debug!("Sending gossip response to {:?}: {:?}", target_url, msg);
    tx.try_send(GossipResponse(
        serialize_gossip_message(msg.into())?,
        target_url,
    ))
    .map_err(|e| K2Error::other_src("could not send response", e))
}

impl From<K2GossipInitiateMessage> for K2GossipMessage {
    fn from(i: K2GossipInitiateMessage) -> Self {
        K2GossipMessage {
            gossip_message: Some(
                crate::protocol::k2_gossip_message::GossipMessage::Initiate(i),
            ),
        }
    }
}

impl From<K2GossipAcceptMessage> for K2GossipMessage {
    fn from(i: K2GossipAcceptMessage) -> Self {
        K2GossipMessage {
            gossip_message: Some(
                crate::protocol::k2_gossip_message::GossipMessage::Accept(i),
            ),
        }
    }
}
