use crate::gossip::K2Gossip;
use crate::protocol::k2_gossip_accept_message::SnapshotMinimalMessage;
use crate::protocol::{
    encode_agent_ids, encode_op_ids, ArcSetMessage, GossipMessage,
    K2GossipAcceptMessage, K2GossipInitiateMessage,
};
use kitsune2_api::{K2Error, K2Result, Timestamp, Url, UNIX_TIMESTAMP};
use kitsune2_dht::ArcSet;

impl K2Gossip {
    pub(super) async fn respond_to_initiate(
        &self,
        from_peer: Url,
        initiate: K2GossipInitiateMessage,
    ) -> K2Result<Option<GossipMessage>> {
        // Rate limit incoming gossip messages by peer
        self.check_initiate_rate(from_peer.clone()).await?;

        // Note the gap between the check and write here. It's possible that both peers
        // could initiate at the same time. This is slightly wasteful but shouldn't be a
        // problem.
        self.peer_meta_store
            .set_last_gossip_timestamp(from_peer.clone(), Timestamp::now())
            .await?;

        let other_arc_set = match &initiate.arc_set {
            Some(message) => ArcSet::decode(&message.value)?,
            None => {
                return Err(K2Error::other("no arc set in initiate message"));
            }
        };

        let (our_agents, our_arc_set) = self.local_agent_state().await?;
        let common_arc_set = our_arc_set.intersection(&other_arc_set);

        // There's no validation to be done with an accept beyond what's been done above
        // to check how recently this peer initiated with us. We'll just record that they
        // have initiated and that we plan to accept.
        self.create_accept_state(
            &from_peer,
            &initiate,
            our_agents.clone(),
            common_arc_set.clone(),
        )
        .await?;

        // Now we can start the work of creating an accept response, starting with a
        // minimal DHT snapshot if there is an arc set overlap.
        let snapshot: Option<SnapshotMinimalMessage> =
            if common_arc_set.covered_sector_count() > 0 {
                let snapshot = self
                    .dht
                    .read()
                    .await
                    .snapshot_minimal(common_arc_set)
                    .await?;
                Some(snapshot.try_into()?)
            } else {
                // TODO Need to decide what to do here. It's useful for now and it's reasonable
                //      to need to initiate to discover this but we do want to minimize work
                //      in this case.
                tracing::info!(
                    "no common arc set, continue to sync agents but not ops"
                );
                None
            };

        let missing_agents = self
            .filter_known_agents(&initiate.participating_agents)
            .await?;

        let new_since = self
            .peer_meta_store
            .new_ops_bookmark(from_peer.clone())
            .await?
            .unwrap_or(UNIX_TIMESTAMP);

        // TODO Use common arc set here to restrict the ops we look up.
        //      Which also means that changing arc will invalidate the bookmark?
        let (new_ops, new_bookmark) = self
            .op_store
            .retrieve_op_ids_bounded(
                Timestamp::from_micros(initiate.new_since),
                initiate.max_new_bytes as usize,
            )
            .await?;

        Ok(Some(GossipMessage::Accept(K2GossipAcceptMessage {
            session_id: initiate.session_id,
            participating_agents: encode_agent_ids(our_agents),
            arc_set: Some(ArcSetMessage {
                value: our_arc_set.encode(),
            }),
            missing_agents,
            new_since: new_since.as_micros(),
            max_new_bytes: self.config.max_gossip_op_bytes,
            new_ops: encode_op_ids(new_ops),
            updated_new_since: new_bookmark.as_micros(),
            snapshot,
        })))
    }

    async fn check_initiate_rate(&self, from_peer: Url) -> K2Result<()> {
        if let Some(timestamp) = self
            .peer_meta_store
            .last_gossip_timestamp(from_peer.clone())
            .await?
        {
            let elapsed = (Timestamp::now() - timestamp).map_err(|_| {
                K2Error::other("could not calculate elapsed time")
            })?;

            if elapsed < self.config.min_initiate_interval() {
                tracing::info!(
                    "Peer [{:?}] attempted to initiate too soon: {:?} < {:?}",
                    from_peer,
                    elapsed,
                    self.config.min_initiate_interval()
                );
                return Err(K2Error::other("initiate too soon"));
            }
        }

        Ok(())
    }
}
