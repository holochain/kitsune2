use crate::gossip::{send_gossip_message, K2Gossip};
use crate::protocol::{
    AcceptResponseMessage, GossipMessage, K2GossipInitiateMessage,
};
use crate::state::GossipRoundState;
use bytes::Bytes;
use kitsune2_api::agent::AgentInfoSigned;
use kitsune2_api::id::decode_ids;
use kitsune2_api::{AgentId, K2Error, K2Result, OpId, Timestamp, Url};
use kitsune2_dht::ArcSet;
use std::sync::Arc;
use tokio::sync::Mutex;

mod accept;
mod agents;
mod busy;
mod disc_sector_details_diff;
mod disc_sector_details_diff_response;
mod disc_sectors_diff;
mod hashes;
mod initiate;
mod no_diff;
mod ring_sector_details_diff;
mod ring_sector_details_diff_response;

#[cfg(test)]
mod harness;

impl K2Gossip {
    pub(super) async fn respond_to_msg(
        &self,
        from_peer: Url,
        msg: GossipMessage,
    ) -> K2Result<()> {
        let res = match msg {
            GossipMessage::Initiate(initiate) => {
                self.respond_to_initiate(from_peer.clone(), initiate).await
            }
            GossipMessage::Accept(accept) => {
                self.respond_to_accept(from_peer.clone(), accept).await
            }
            GossipMessage::NoDiff(no_diff) => {
                self.respond_to_no_diff(from_peer.clone(), no_diff).await
            }
            GossipMessage::DiscSectorsDiff(disc_sectors_diff) => {
                self.respond_to_disc_sectors_diff(
                    from_peer.clone(),
                    disc_sectors_diff,
                )
                .await
            }
            GossipMessage::DiscSectorDetailsDiff(disc_sector_details_diff) => {
                self.respond_to_disc_sector_details_diff(
                    from_peer.clone(),
                    disc_sector_details_diff,
                )
                .await
            }
            GossipMessage::DiscSectorDetailsDiffResponse(
                disc_sector_details_response_diff,
            ) => {
                self.respond_to_disc_sector_details_diff_response(
                    from_peer.clone(),
                    disc_sector_details_response_diff,
                )
                .await
            }
            GossipMessage::RingSectorDetailsDiff(ring_sector_details_diff) => {
                self.respond_to_ring_sector_details_diff(
                    from_peer.clone(),
                    ring_sector_details_diff,
                )
                .await
            }
            GossipMessage::RingSectorDetailsDiffResponse(
                ring_sector_details_diff_response,
            ) => {
                self.respond_to_ring_sector_details_diff_response(
                    from_peer.clone(),
                    ring_sector_details_diff_response,
                )
                .await
            }
            GossipMessage::Hashes(hashes) => {
                self.respond_to_hashes(from_peer.clone(), hashes).await
            }
            GossipMessage::Agents(agents) => {
                self.respond_to_agents(from_peer.clone(), agents).await
            }
            GossipMessage::Busy(busy) => {
                self.respond_to_busy(from_peer.clone(), busy).await?;
                Ok(None)
            }
        }?;

        if let Some(msg) = res {
            send_gossip_message(&self.response_tx, from_peer, msg)?;
        }

        Ok(())
    }

    pub(crate) async fn create_accept_state(
        &self,
        from_peer: &Url,
        initiate: &K2GossipInitiateMessage,
        our_agents: Vec<AgentId>,
        common_arc_set: ArcSet,
    ) -> K2Result<()> {
        let mut accepted_states = self.accepted_round_states.write().await;
        let accepted_entry = accepted_states.entry(from_peer.clone());
        match accepted_entry {
            std::collections::hash_map::Entry::Occupied(_) => {
                return Err(K2Error::other(format!(
                    "peer {:?} already accepted",
                    from_peer
                )));
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Arc::new(Mutex::new(
                    GossipRoundState::new_accepted(
                        from_peer.clone(),
                        initiate.session_id.clone(),
                        our_agents,
                        common_arc_set,
                    ),
                )));
            }
        }

        Ok(())
    }

    pub(crate) async fn handle_accept_response(
        &self,
        from_peer: &Url,
        accept_response: AcceptResponseMessage,
    ) -> K2Result<Option<Vec<Arc<AgentInfoSigned>>>> {
        self.receive_agent_infos(accept_response.provided_agents)
            .await?;

        self.update_new_ops_bookmark(
            from_peer.clone(),
            Timestamp::from_micros(accept_response.updated_new_since),
        )
        .await?;

        self.fetch
            .request_ops(decode_ids(accept_response.new_ops), from_peer.clone())
            .await?;
        self.peer_meta_store
            .set_new_ops_bookmark(
                from_peer.clone(),
                Timestamp::from_micros(accept_response.updated_new_since),
            )
            .await?;

        if accept_response.missing_agents.is_empty() {
            Ok(None)
        } else {
            let send_agent_infos =
                self.load_agent_infos(accept_response.missing_agents).await;

            Ok(Some(send_agent_infos))
        }
    }

    /// Filter out agents that are already known and return a list of unknown agents.
    ///
    /// This is useful when receiving a list of agents from a peer, and we want to filter out
    /// the ones we already know about. The resulting list should be sent back as a request
    /// to get infos for the unknown agents.
    pub(crate) async fn filter_known_agents<T: Into<AgentId> + Clone>(
        &self,
        agents: &[T],
    ) -> K2Result<Vec<T>> {
        let mut out = Vec::new();
        for agent in agents {
            let agent_id = agent.clone().into();
            if self.peer_store.get(agent_id).await?.is_none() {
                out.push(agent.clone());
            }
        }

        Ok(out)
    }

    /// Load agent infos from the peer store.
    ///
    /// Loads any of the requested agents that are available in the peer store.
    pub(crate) async fn load_agent_infos<T: Into<AgentId> + Clone>(
        &self,
        requested: Vec<T>,
    ) -> Vec<Arc<AgentInfoSigned>> {
        if requested.is_empty() {
            return vec![];
        }

        let mut agent_infos = vec![];
        for missing_agent in requested {
            if let Ok(Some(agent_info)) =
                self.peer_store.get(missing_agent.clone().into()).await
            {
                agent_infos.push(agent_info);
            }
        }

        agent_infos
    }

    /// Receive agent info messages from the network.
    ///
    /// Each info is checked against the verifier and then stored in the peer store.
    pub(crate) async fn receive_agent_infos(
        &self,
        provided_agents: Vec<Bytes>,
    ) -> K2Result<()> {
        if provided_agents.is_empty() {
            return Ok(());
        }

        // TODO check that the incoming agents are the one we requested
        let mut agents = Vec::with_capacity(provided_agents.len());
        for agent in provided_agents {
            let agent_info =
                AgentInfoSigned::decode(&self.agent_verifier, &agent)?;
            agents.push(agent_info);
        }
        tracing::info!("Storing agents: {:?}", agents);
        self.peer_store.insert(agents).await?;

        Ok(())
    }

    pub(crate) async fn local_agent_state(
        &self,
    ) -> K2Result<(Vec<AgentId>, ArcSet)> {
        let local_agents = self.local_agent_store.get_all().await?;
        let (send_agents, our_arcs) = local_agents
            .iter()
            .map(|a| (a.agent().clone(), a.get_tgt_storage_arc()))
            .collect::<(Vec<_>, Vec<_>)>();

        let our_arc_set = ArcSet::new(our_arcs)?;

        Ok((send_agents, our_arc_set))
    }

    pub(crate) async fn update_new_ops_bookmark(
        &self,
        from_peer: Url,
        updated_bookmark: Timestamp,
    ) -> K2Result<()> {
        let previous_bookmark = self
            .peer_meta_store
            .new_ops_bookmark(from_peer.clone())
            .await?;

        if previous_bookmark
            .map(|previous_bookmark| previous_bookmark <= updated_bookmark)
            .unwrap_or(true)
        {
            self.peer_meta_store
                .set_new_ops_bookmark(from_peer.clone(), updated_bookmark)
                .await?;
        } else {
            // This could happen due to a clock issue. If it happens frequently, or by a
            // large margin, it could be a sign of malicious activity.
            tracing::warn!(
                "new bookmark is older than previous bookmark from peer: {:?}",
                from_peer
            );
        }

        Ok(())
    }

    pub(crate) async fn retrieve_new_op_ids(
        &self,
        common_arc_set: &ArcSet,
        new_since: Timestamp,
        max_new_bytes: usize,
    ) -> K2Result<(Vec<OpId>, Timestamp)> {
        let mut used_bytes = 0;
        let mut send_new_ops = Vec::new();
        let mut send_new_bookmark = Timestamp::now();

        for arc in common_arc_set.as_arcs() {
            let (new_ops, used, new_bookmark) = self
                .op_store
                .retrieve_op_ids_bounded(
                    arc,
                    new_since,
                    max_new_bytes - used_bytes,
                )
                .await?;

            send_new_ops.extend(new_ops);
            used_bytes += used;
            if new_bookmark < send_new_bookmark {
                send_new_bookmark = new_bookmark;
            }
        }

        Ok((send_new_ops, send_new_bookmark))
    }
}
