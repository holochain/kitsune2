use crate::gossip::K2Gossip;
use kitsune2_api::{DhtArc, K2Error, K2Result, LocalAgent, Timestamp, Url};
use kitsune2_dht::{ArcSet, DhtSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// DHT segment state.
///
/// See [`DhtSnapshot::Minimal`]
#[derive(Debug, Serialize, Deserialize)]
pub struct DhtSegmentState {
    /// The top hash of the DHT ring segment.
    pub disc_top_hash: bytes::Bytes,
    /// The boundary timestamp of the DHT ring segment.
    pub disc_boundary: Timestamp,
    /// The top hashes of each DHT ring segment.
    pub ring_top_hashes: Vec<bytes::Bytes>,
}

/// Peer metadata dump.
#[derive(Debug, Serialize, Deserialize)]
pub struct PeerMeta {
    /// The timestamp of the last gossip round.
    pub last_gossip_timestamp: Option<Timestamp>,
    /// The bookmark of the last op bookmark received.
    pub new_ops_bookmark: Option<Timestamp>,
    /// The number of behavior errors observed.
    pub peer_behavior_errors: Option<u32>,
    /// The number of local errors.
    pub local_errors: Option<u32>,
    /// The number of busy peer errors.
    pub peer_busy: Option<u32>,
    /// The number of terminated rounds.
    ///
    /// Note that termination is not necessarily an error.
    pub peer_terminated: Option<u32>,
    /// The number of completed rounds.
    pub completed_rounds: Option<u32>,
    /// The number of peer timeouts.
    pub peer_timeouts: Option<u32>,
}

/// Gossip round state summary.
#[derive(Debug, Serialize, Deserialize)]
pub struct GossipRoundStateSummary {
    /// The URL of the peer with which the round is initiated.
    pub session_with_peer: Url,
}

/// Gossip state summary.
#[derive(Debug, Serialize, Deserialize)]
pub struct GossipSummary {
    /// The current initiated round summary.
    pub initiated_round: Option<GossipRoundStateSummary>,
    /// The list of accepted round summaries.
    pub accepted_rounds: Vec<GossipRoundStateSummary>,
    /// DHT summary.
    pub dht_summary: HashMap<String, DhtSegmentState>,
    /// Peer metadata dump for each agent in this space.
    pub peer_meta: HashMap<Url, PeerMeta>,
}

impl K2Gossip {
    pub async fn summary(
        &self,
        include_dht_summary: bool,
    ) -> K2Result<serde_json::Value> {
        let mut summary = GossipSummary {
            initiated_round: None,
            accepted_rounds: Vec::new(),
            dht_summary: HashMap::new(),
            peer_meta: HashMap::new(),
        };

        if let Some(current_round) =
            self.initiated_round_state.lock().await.as_ref()
        {
            summary.initiated_round = Some(GossipRoundStateSummary {
                session_with_peer: current_round.session_with_peer.clone(),
            });
        }

        {
            let accepted_states = self.accepted_round_states.read().await;
            for url in accepted_states.keys() {
                summary.accepted_rounds.push(GossipRoundStateSummary {
                    session_with_peer: url.clone(),
                })
            }
        }

        let local_agents = self.local_agent_store.get_all().await?;

        if include_dht_summary {
            let current_arc_set = ArcSet::new(
                local_agents
                    .iter()
                    .map(|l| l.get_tgt_storage_arc())
                    .collect(),
            )?;

            for arc in current_arc_set.as_arcs() {
                let arc_set = ArcSet::new(vec![arc])?;

                let snapshot: DhtSnapshot = self
                    .dht
                    .read()
                    .await
                    .snapshot_minimal(arc_set.clone())
                    .await?;
                match snapshot {
                    DhtSnapshot::Minimal {
                        disc_top_hash,
                        disc_boundary,
                        ring_top_hashes,
                    } => {
                        summary.dht_summary.insert(
                            match arc_set.as_arcs().first().ok_or_else(
                                || K2Error::other("empty arc set"),
                            )? {
                                DhtArc::Arc(start, end) => {
                                    format!("{}..{}", start, end)
                                }
                                DhtArc::Empty => {
                                    return Err(K2Error::other("empty arc"))
                                }
                            },
                            DhtSegmentState {
                                disc_top_hash,
                                disc_boundary,
                                ring_top_hashes,
                            },
                        );
                    }
                    _ => {
                        return Err(K2Error::other("unexpected snapshot type"))
                    }
                }
            }
        }

        let agents = self.peer_store.get_all().await?;
        for agent in agents {
            if local_agents.iter().any(|l| l.agent() == &agent.agent) {
                continue;
            }

            let Some(url) = agent.url.clone() else {
                continue;
            };

            summary.peer_meta.insert(
                url.clone(),
                PeerMeta {
                    last_gossip_timestamp: self
                        .peer_meta_store
                        .last_gossip_timestamp(url.clone())
                        .await?,
                    new_ops_bookmark: self
                        .peer_meta_store
                        .new_ops_bookmark(url.clone())
                        .await?,
                    peer_behavior_errors: self
                        .peer_meta_store
                        .peer_behavior_errors(url.clone())
                        .await?,
                    local_errors: self
                        .peer_meta_store
                        .local_errors(url.clone())
                        .await?,
                    peer_busy: self
                        .peer_meta_store
                        .peer_busy(url.clone())
                        .await?,
                    peer_terminated: self
                        .peer_meta_store
                        .peer_terminated(url.clone())
                        .await?,
                    completed_rounds: self
                        .peer_meta_store
                        .completed_rounds(url.clone())
                        .await?,
                    peer_timeouts: self
                        .peer_meta_store
                        .peer_timeouts(url.clone())
                        .await?,
                },
            );
        }

        serde_json::to_value(summary)
            .map_err(|e| K2Error::other_src("Failed to serialize summary", e))
    }
}
