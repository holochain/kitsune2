use crate::types::{GossipMessage, PeerHello};
use kitsune2_api::{AgentId, BoundedOpHashResponse, HostApi, OpId, PeerMeta, Timestamp};
use std::collections::HashSet;

pub mod types;

pub async fn initiate_gossip_round<E: std::error::Error + 'static>(
    host_api: HostApi<E>,
    with_agent: AgentId,
) -> Result<GossipMessage, E> {
    let chunk_states = vec![];

    let peer_meta = host_api.peer_meta_store.get_peer_meta(with_agent).await?;

    let hello = PeerHello {
        chunk_states,
        new_ops_since: peer_meta
            .as_ref()
            .and_then(|pm| pm.last_gossip_timestamp)
            .unwrap_or(Timestamp::from_micros(0)),
        new_ops_max_bytes: 1024,
    };

    Ok(GossipMessage::PeerInitiate(hello))
}

pub async fn respond_to_gossip_message<E: std::error::Error + 'static>(
    host_api: HostApi<E>,
    from_agent: AgentId,
    message: GossipMessage,
) -> Result<Option<GossipMessage>, E> {
    match message {
        GossipMessage::PeerInitiate(hello) => {
            respond_to_initiate(host_api, from_agent, hello)
                .await
                .map(Some)
        }
        GossipMessage::PeerAccept {
            peer_hello,
            new_op_hashes,
            bookmark_timestamp,
        } => {
            respond_to_accept(
                host_api,
                from_agent,
                peer_hello,
                new_op_hashes,
                bookmark_timestamp,
            )
            .await?
        }
        GossipMessage::PeerCompleteAfterAccept {
            bookmark_timestamp,
            ..
        } => {
            handle_complete_after_accept(
                host_api,
                from_agent,
                bookmark_timestamp,
            )
            .await?
        }
    }
}

async fn respond_to_initiate<E: std::error::Error + 'static>(
    host_api: HostApi<E>,
    from_agent: AgentId,
    hello: PeerHello,
) -> Result<GossipMessage, E> {
    // TODO check incoming peer hello

    let chunk_states = vec![];

    let peer_meta = host_api.peer_meta_store.get_peer_meta(from_agent).await?;

    let default_bookmark_timestamp = Timestamp::now();
    let BoundedOpHashResponse { op_ids: new_op_hashes, last_timestamp: bookmark_timestamp } = host_api
        .op_store
        .retrieve_op_hashes(hello.new_ops_since, hello.new_ops_max_bytes)
        .await?;

    let hello = PeerHello {
        chunk_states,
        new_ops_since: peer_meta
            .as_ref()
            .and_then(|pm| pm.last_gossip_timestamp)
            .unwrap_or(Timestamp::from_micros(0)),
        new_ops_max_bytes: 1024,
    };

    Ok(GossipMessage::PeerAccept {
        peer_hello: hello,
        new_op_hashes,
        bookmark_timestamp: bookmark_timestamp
            .unwrap_or(default_bookmark_timestamp),
    })
}

async fn respond_to_accept<E: std::error::Error + 'static>(
    host_api: HostApi<E>,
    from_agent: AgentId,
    peer_hello: PeerHello,
    new_op_hashes: Vec<OpId>,
    bookmark_timestamp: Timestamp,
) -> Result<Result<Option<GossipMessage>, E>, E> {
    // TODO check incoming peer hello

    // TODO send new ops to the fetch pool

    // After we've queued up those ops to be fetched, we can update the peer meta store
    // TODO what happens if we crash now? The fetch pool isn't persistent
    //      Or do we have to just figure out where we're up to if we restart?
    let mut peer_meta = host_api
        .peer_meta_store
        .get_peer_meta(from_agent.clone())
        .await?
        .unwrap_or_else(|| PeerMeta::new(from_agent));
    peer_meta.last_gossip_timestamp = Some(bookmark_timestamp);
    host_api.peer_meta_store.store_peer_meta(peer_meta).await?;

    let default_bookmark_timestamp = Timestamp::now();
    let BoundedOpHashResponse { op_ids: our_new_ops, last_timestamp: our_bookmark_timestamp } = host_api
        .op_store
        .retrieve_op_hashes(
            peer_hello.new_ops_since,
            peer_hello.new_ops_max_bytes,
        )
        .await?;

    // Don't send back any op hashes that the other peer just sent us, because they already
    // know about those.
    let our_new_ops = our_new_ops
        .into_iter()
        .collect::<HashSet<_>>()
        .difference(&new_op_hashes.into_iter().collect())
        .cloned()
        .collect::<Vec<_>>();

    Ok(Ok(Some(GossipMessage::PeerCompleteAfterAccept {
        new_op_hashes: our_new_ops,
        bookmark_timestamp: our_bookmark_timestamp
            .unwrap_or(default_bookmark_timestamp),
    })))
}

async fn handle_complete_after_accept<E: std::error::Error + 'static>(
    host_api: HostApi<E>,
    from_agent: AgentId,
    bookmark_timestamp: Timestamp,
) -> Result<Result<Option<GossipMessage>, E>, E> {
    // TODO send new ops to the fetch pool

    // After we've queued up those ops to be fetched, we can update the peer meta store
    // TODO what happens if we crash now? The fetch pool isn't persistent
    //      Or do we have to just figure out where we're up to if we restart?
    let mut peer_meta = host_api
        .peer_meta_store
        .get_peer_meta(from_agent.clone())
        .await?
        .unwrap_or_else(|| PeerMeta::new(from_agent));
    peer_meta.last_gossip_timestamp = Some(bookmark_timestamp);
    host_api.peer_meta_store.store_peer_meta(peer_meta).await?;

    Ok(Ok(None))
}
