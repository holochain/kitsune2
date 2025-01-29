use crate::gossip::{send_gossip_message, K2Gossip};
use crate::protocol::GossipMessage;
use kitsune2_api::{K2Result, Url};

mod accept;
mod agents;
mod disc_sector_details_diff;
mod disc_sector_details_diff_response;
mod disc_sectors_diff;
mod hashes;
mod initiate;
mod no_diff;
mod ring_sector_details_diff;
mod ring_sector_details_diff_response;

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
        }?;

        if let Some(msg) = res {
            send_gossip_message(&self.response_tx, from_peer, msg)?;
        }

        Ok(())
    }
}
