use crate::protocol::{serialize_gossip_message, GossipMessage};
use kitsune2_api::agent::LocalAgent;
use kitsune2_api::space::DynSpace;
use kitsune2_api::{AgentId, K2Error, K2Result, Url};
use kitsune2_dht::ArcSet;
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

    let our_arc_set = ArcSet::new(our_arcs)?;

    Ok((send_agents, our_arc_set))
}

pub(crate) fn send_gossip_message(
    tx: &Sender<GossipResponse>,
    target_url: Url,
    msg: GossipMessage,
) -> K2Result<()> {
    tracing::debug!("Sending gossip response to {:?}: {:?}", target_url, msg);
    tx.try_send(GossipResponse(serialize_gossip_message(msg)?, target_url))
        .map_err(|e| K2Error::other_src("could not send response", e))
}
