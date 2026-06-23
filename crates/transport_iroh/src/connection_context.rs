use crate::IrohTransport;
use crate::SpaceRelays;
use crate::connection::DynConnection;
#[cfg(feature = "metrics")]
use crate::metrics::connection_counter_metric;
use crate::stream::{DynIrohRecvStream, DynIrohSendStream};
use crate::{
    Connections, FRAME_HEADER_LEN, FrameType, decode_frame_header,
    decode_frame_preflight,
    frame::{Frame, encode_frame},
};
use bytes::Bytes;
use kitsune2_api::{K2Error, K2Result, Timestamp, TxImpHnd, Url};
use std::{
    fmt,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{sync::MutexGuard, task::AbortHandle};
use tracing::{debug, error, info, trace, warn};

pub(super) struct ConnectionContext {
    handler: Arc<TxImpHnd>,
    connection: DynConnection,
    /// Our own endpoint id. Compared against the remote id to resolve
    /// simultaneous-open races deterministically (see
    /// [`ConnectionContext::is_preferred_connection`]).
    local_id: [u8; 32],
    /// Whether we dialed this connection (`true`) or accepted it (`false`).
    /// Part of the simultaneous-open tie-break.
    dialed_by_us: bool,
    connection_reader_abort_handle: Mutex<Option<AbortHandle>>,
    send_stream: tokio::sync::Mutex<Option<DynIrohSendStream>>,
    remote_url: RwLock<Option<Url>>,
    preflight_sent: AtomicBool,
    preflight_received: AtomicBool,
    send_message_count: AtomicU64,
    send_bytes: AtomicU64,
    recv_message_count: AtomicU64,
    recv_bytes: AtomicU64,
    opened_at_s: u64,
    max_frame_bytes: usize,
    space_relays: SpaceRelays,
}

impl fmt::Debug for ConnectionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionContext").finish()
    }
}

pub(super) struct ConnectionContextParams {
    pub handler: Arc<TxImpHnd>,
    pub connection: DynConnection,
    pub local_id: [u8; 32],
    pub dialed_by_us: bool,
    pub remote_url: Option<Url>,
    pub preflight_sent: bool,
    pub opened_at_s: u64,
    pub connections: Connections,
    pub local_url: Arc<RwLock<Option<Url>>>,
    pub space_relays: SpaceRelays,
    pub max_frame_bytes: usize,
}

impl ConnectionContext {
    pub fn new(params: ConnectionContextParams) -> Arc<Self> {
        let ctx = Arc::new(Self {
            handler: params.handler,
            connection: params.connection,
            local_id: params.local_id,
            dialed_by_us: params.dialed_by_us,
            connection_reader_abort_handle: Mutex::new(None),
            send_stream: tokio::sync::Mutex::new(None),
            remote_url: RwLock::new(params.remote_url),
            preflight_sent: AtomicBool::new(params.preflight_sent),
            preflight_received: AtomicBool::new(false),
            send_message_count: AtomicU64::new(0),
            send_bytes: AtomicU64::new(0),
            recv_message_count: AtomicU64::new(0),
            recv_bytes: AtomicU64::new(0),
            opened_at_s: params.opened_at_s,
            max_frame_bytes: params.max_frame_bytes,
            space_relays: params.space_relays,
        });

        // Spawn connection reader to listen for incoming connections on the
        // new connection.
        let connection_reader_abort_handle = Self::spawn_connection_reader(
            ctx.clone(),
            params.connections,
            params.local_url,
        );
        *ctx.connection_reader_abort_handle.lock().expect("poisoned") =
            Some(connection_reader_abort_handle);

        ctx
    }

    pub async fn send_preflight_frame(
        &self,
        url: Url,
        preflight_bytes: Bytes,
    ) -> K2Result<()> {
        let frame = encode_frame(
            Frame::Preflight((url.clone(), preflight_bytes)),
            self.max_frame_bytes,
        )?;

        let mut stream_lock = self.ensure_send_stream().await?;
        let stream = stream_lock.as_mut().expect("stream must exist");

        info!(local_url = ?url, "Sending preflight frame");
        trace!(?frame, "Sending preflight frame");
        if let Err(err) = stream.write_all(&frame).await {
            error!(?err, "Failed to send preflight frame");
            *stream_lock = None;
            return Err(err);
        }

        Ok(())
    }

    pub async fn send_data_frame(&self, data: Bytes) -> K2Result<()> {
        let data_len = data.len() as u64;
        let frame = encode_frame(Frame::Data(data), self.max_frame_bytes)?;

        let mut stream_lock = self.ensure_send_stream().await?;
        let stream = stream_lock.as_mut().expect("stream must exist");

        trace!(?frame, "Sending data frame");
        if let Err(err) = stream.write_all(&frame).await {
            error!(?err, "Failed to send data frame");
            *stream_lock = None;
            return Err(err);
        }

        drop(stream_lock);

        // Update stats
        self.increment_send_message_count();
        self.increment_send_bytes(data_len);

        Ok(())
    }

    pub fn remote_url(&self) -> Option<Url> {
        self.remote_url.read().expect("poisoned").clone()
    }

    pub fn get_send_message_count(&self) -> u64 {
        self.send_message_count.load(Ordering::SeqCst)
    }

    pub fn get_send_bytes(&self) -> u64 {
        self.send_bytes.load(Ordering::SeqCst)
    }

    pub fn get_recv_message_count(&self) -> u64 {
        self.recv_message_count.load(Ordering::SeqCst)
    }

    pub fn get_recv_bytes(&self) -> u64 {
        self.recv_bytes.load(Ordering::SeqCst)
    }

    pub fn get_opened_at_s(&self) -> u64 {
        self.opened_at_s
    }

    /// Check if the connection's selected path is direct (IP-based, non-relay).
    pub fn is_direct(&self) -> bool {
        self.connection.is_direct()
    }

    /// Whether this connection is the one both peers should converge on when a
    /// simultaneous dial has produced two connections between the same pair.
    ///
    /// The tie-break is deterministic: keep the connection dialed by the
    /// endpoint with the larger id. Both peers evaluate this with the same two
    /// ids and the same notion of who dialed, so they always agree on the same
    /// physical connection — the loser can then be closed without tearing down
    /// the survivor.
    fn is_preferred_connection(&self) -> bool {
        let remote_id = *self.connection.remote_id().as_bytes();
        let larger_endpoint_dialed = self.local_id > remote_id;
        self.dialed_by_us == larger_endpoint_dialed
    }

    /// Register `self` as the active connection for `remote_url`, resolving any
    /// simultaneous-open race deterministically.
    ///
    /// - If the slot is free, `self` takes it.
    /// - If `self` is already the active connection, this is a no-op.
    /// - If a *different* connection to the same peer already exists, the
    ///   [`is_preferred_connection`](Self::is_preferred_connection) tie-break
    ///   decides which survives. When `self` wins, the displaced connection is
    ///   closed (not aborted) so its reader observes the close and exits
    ///   quietly through the identity-aware cleanup path. When `self` loses,
    ///   the existing connection is left in place.
    ///
    /// Returns `true` if `self` is the active connection afterwards, `false` if
    /// it lost the tie-break and should be torn down.
    pub(super) fn register_as_active(
        self: &Arc<Self>,
        connections: &Connections,
        remote_url: &Url,
    ) -> bool {
        let displaced = {
            let mut map = connections.write().expect("poisoned");
            match map.get(remote_url) {
                Some(existing) if Arc::ptr_eq(existing, self) => return true,
                Some(_) if !self.is_preferred_connection() => return false,
                // Slot is free, or we win the tie-break against the existing
                // connection: take the slot.
                _ => map.insert(remote_url.clone(), self.clone()),
            }
        };

        // We are now the active connection.
        #[cfg(feature = "metrics")]
        connection_counter_metric().add(1, &[]);

        if let Some(displaced) = displaced {
            // The displaced connection was previously counted as active.
            #[cfg(feature = "metrics")]
            connection_counter_metric().add(-1, &[]);
            displaced
                .connection
                .close(0u8, b"superseded by preferred connection");
        }

        true
    }

    pub fn abort_tasks(&self) {
        if let Some(abort_handle) = self
            .connection_reader_abort_handle
            .lock()
            .expect("poisoned")
            .take()
        {
            abort_handle.abort();
        }
    }

    /// Close the underlying connection without notifying the handler.
    ///
    /// Used to discard a redundant connection that lost simultaneous-open
    /// resolution. The connection's reader then observes the close and exits
    /// through the identity-aware cleanup path, which sees that this is not the
    /// active connection and so does not fire `peer_disconnect`.
    pub(super) fn close_quietly(&self) {
        self.connection
            .close(0u8, b"superseded by preferred connection");
    }

    pub fn disconnect(&self, reason: String) {
        info!(reason, remote_url = ?self.remote_url(), "Disconnecting from remote");
        self.connection.close(0u8, reason.as_bytes());
        if let Some(peer) = self.remote_url() {
            self.handler.peer_disconnect(peer, Some(reason));
            // Record connection counter metric.
            #[cfg(feature = "metrics")]
            connection_counter_metric().add(-1, &[]);
        }
    }

    // Spawns an asynchronous task to continuously read and handle incoming uni-directional
    // streams from an iroh connection. There is only one stream at a time incoming from a
    // remote. It's read from until the connection is closed or an error occurs.
    //
    // Errors when receiving the preflight frame lead to a break of the loop accepting
    // incoming streams. The preflight must succeed for data frames to be accepted. The
    // connection cannot recover from a failed preflight, and a new connection must be
    // established.
    //
    // # Parameters
    // - `ctx`: The connection context containing handler and remote URL state.
    // - `connections`: Shared map of peer URLs to their connection contexts, updated when
    //   the preflight succeeds.
    // - `local_url`: The local URL for this endpoint, used to respond to preflight messages.
    fn spawn_connection_reader(
        ctx: Arc<Self>,
        connections: Connections,
        local_url: Arc<RwLock<Option<Url>>>,
    ) -> AbortHandle {
        tokio::spawn(async move {
            // Track whether to skip marking peer as unresponsive.
            // This is set to true for temporary errors like NoLocalAgentsDuringPreflight,
            // which indicate a timing issue rather than a network problem.
            let mut skip_unresponsive = false;

            let err = loop {
                // Main loop to accept incoming unidirectional streams from the remote peer.
                match ctx.connection.accept_uni().await {
                    Ok(stream) => {
                        info!(remote_id = ?ctx.connection.remote_id(), "Accepted incoming stream");
                        let connections = connections.clone();
                        let local_url = local_url.clone();
                        // Read frames from the stream.
                        //
                        // `Ok(true)` keeps the connection open and awaits the next
                        // incoming stream — returned both after a successful preflight
                        // and after a data stream ends normally.
                        //
                        // `Ok(false)` means this connection lost simultaneous-open
                        // resolution: a preferred connection to the same peer is already
                        // active, so this reader stops quietly.
                        //
                        // `Err` means the preflight could not be received. The connection
                        // must be closed, because a successful preflight is the
                        // prerequisite for establishing a connection.
                        match Self::handle_incoming_stream(
                            ctx.clone(),
                            stream,
                            connections.clone(),
                            local_url,
                        )
                            .await
                        {
                            Ok(true) => {}
                            Ok(false) => {
                                break "superseded by preferred connection".to_string();
                            }
                            Err(err) => {
                                // Don't mark peer as unresponsive for NoLocalAgentsDuringPreflight
                                // errors - this is a temporary state that will resolve once an
                                // agent joins. It is not a real failure, so log it quietly and
                                // reserve `error!` for genuine preflight failures.
                                if matches!(err, K2Error::NoLocalAgentsDuringPreflight) {
                                    skip_unresponsive = true;
                                    debug!(?err, "Stream closed during preflight; no local agents yet");
                                } else {
                                    error!(?err, "Stream closed by remote");
                                }
                                break err.to_string();
                            }
                        }
                    }
                    Err(err) => {
                        error!(?err, "Connection closed by remote");
                        break err.to_string();
                    }
                }
            };

            // The reader loop has ended. Only the connection that is *still the
            // active connection* for this peer performs the "peer is gone"
            // cleanup — marking the peer unresponsive and firing
            // `peer_disconnect`.
            //
            // A connection that was superseded during simultaneous-open
            // resolution, or already replaced by a newer connection, is no
            // longer the map entry for this peer. It closes quietly so that
            // tearing it down does not tear down the surviving connection.
            if let Some(remote_url) = ctx.remote_url() {
                let was_active = {
                    let mut map = connections.write().expect("poisoned");
                    match map.get(&remote_url) {
                        Some(active) if Arc::ptr_eq(active, &ctx) => {
                            map.remove(&remote_url);
                            true
                        }
                        _ => false,
                    }
                };

                if was_active {
                    if !skip_unresponsive {
                        info!(?remote_url, "Setting peer unresponsive");
                        if let Err(err) = ctx.handler.set_unresponsive(remote_url.clone(), Timestamp::now()).await {
                            warn!(?err, ?remote_url, "Failed to set peer unresponsive");
                        }
                    } else {
                        info!(?remote_url, "Skipping set_unresponsive due to temporary error (no local agents)");
                    }
                    ctx.disconnect(err);
                } else {
                    debug!(?remote_url, reason = %err, "Connection reader stopped; not the active connection, closing quietly");
                    ctx.connection.close(0u8, b"superseded connection");
                }
            } else {
                // Preflight never completed, so no peer URL was learned and the
                // peer was never surfaced to the handler.
                ctx.connection.close(0u8, err.as_bytes());
            }
        }).abort_handle()
    }

    // Handle frames from an incoming stream.
    //
    // By convention, the first frame on a new connection is the
    // preflight. After the preflight has been received, the flag is
    // updated in the context.
    //
    // If the preflight has not been received yet, read the preflight
    // from the stream. Time out if the preflight isn't received and
    // return an error to close the stream.
    //
    // The protocol can't recover from a failed preflight frame.
    // The stream must be closed with an error, which causes the
    // connection to be closed. A new connection must be established
    // and the preflight has to be sent again.
    //
    // Once the preflight frame has been successfully received, data
    // frames can be read from the stream. No other frames are allowed
    // after the preflight.
    //
    // Data frames will be read from the stream until an error of any
    // kind occurs. Errors during data frame header or data reception
    // or decoding will close the stream, but not the connection.
    // The connection reader will await the next incoming stream.
    // Returns `Ok(true)` to keep the connection open and await the next stream,
    // `Ok(false)` if this connection lost simultaneous-open resolution and the
    // reader should stop, or `Err` if the preflight could not be received.
    async fn handle_incoming_stream(
        ctx: Arc<Self>,
        recv_stream: DynIrohRecvStream,
        connections: Connections,
        local_url: Arc<RwLock<Option<Url>>>,
    ) -> K2Result<bool> {
        if !ctx.preflight_received() {
            let result = tokio::time::timeout(Duration::from_secs(10), async {
                let (remote_url, preflight_bytes) = read_preflight_frame_from_stream(&recv_stream, ctx.max_frame_bytes).await?;

                ctx.set_remote_url(remote_url.clone());
                ctx.handler
                    .recv_data(remote_url.clone(), preflight_bytes)
                    .await?;
                ctx.set_preflight_received();
                info!(remote = ?remote_url.peer_id(),"Preflight received successfully");

                // If the preflight has not been sent yet, it must be the first message
                // sent back to the remote.
                if !ctx.preflight_sent() {
                    let global_url = local_url.read().expect("poisoned").clone();
                    let space_relays = ctx.space_relays.read().expect("poisoned").clone();
                    let own_url = IrohTransport::own_url_for_preflight(
                        &remote_url,
                        &space_relays,
                        &global_url,
                    );
                    if let Some(own_url) = own_url {
                        let return_preflight =
                            ctx.handler.peer_connect(remote_url.clone()).await?;
                        ctx.send_preflight_frame(
                            own_url.clone(),
                            return_preflight,
                        )
                            .await?;
                        info!(peer = ?ctx.connection.remote_id(), ?own_url, "Sent preflight to peer");
                        ctx.set_preflight_sent();
                    } else {
                        warn!(peer = ?ctx.connection.remote_id(), "Received preflight, but cannot return preflight because own URL is unknown");
                        return Err(K2Error::other("Connection received before home relay URL is known"));
                    }
                }

                Ok(remote_url)
            })
                .await
                .map_err(|err| {
                    K2Error::other_src("timed out waiting for preflight", err)
                });
            match result {
                Ok(Ok(remote_url)) => {
                    // Register as the active connection, resolving any
                    // simultaneous-open race with another connection to the
                    // same peer. If this connection lost the tie-break, stop
                    // reading it; the preferred connection is already active.
                    if !ctx.register_as_active(&connections, &remote_url) {
                        debug!(
                            remote = ?remote_url.peer_id(),
                            "Connection superseded by preferred connection to same peer"
                        );
                        return Ok(false);
                    }
                }
                Ok(Err(err)) | Err(err) => {
                    error!(?err, "failed to receive preflight frame");
                    return Err(err);
                }
            }
        }

        // Keep reading data frames from the stream until it is closed.
        loop {
            let (data, data_len) = match read_data_frame_from_stream(
                &recv_stream,
                ctx.max_frame_bytes,
            )
            .await
            {
                Ok(data) => data,
                Err(err) => {
                    error!(?err, remote = ?ctx.remote_url(), "error receiving data frame");
                    // Frame header could not be read or decoded, wrong frame type
                    // or data frame data could not be read.
                    // Break the loop to close the stream, but not the connection.
                    break;
                }
            };

            // Handle data frame: forward data to handler if remote URL is set.
            let peer = ctx.remote_url().ok_or_else(|| {
                K2Error::other("received data before preflight")
            })?;
            if let Err(err) = ctx
                .handler
                .recv_data(peer.clone(), Bytes::copy_from_slice(&data))
                .await
            {
                error!(?err, remote = ?peer.peer_id(),"error in recv_data");
            };

            ctx.increment_recv_message_count();
            ctx.increment_recv_bytes(data_len as u64);
        }

        // The stream ended; keep the connection open for the next stream.
        Ok(true)
    }

    async fn ensure_send_stream(
        &'_ self,
    ) -> K2Result<MutexGuard<'_, Option<DynIrohSendStream>>> {
        // Atomically open a new stream if none is present.
        let mut stream_lock = self.send_stream.lock().await;
        if stream_lock.is_none() {
            let stream = self.connection.open_uni().await?;
            *stream_lock = Some(stream);
        }
        Ok(stream_lock)
    }

    fn set_remote_url(&self, peer: Url) {
        *self.remote_url.write().expect("poisoned") = Some(peer);
    }

    fn preflight_sent(&self) -> bool {
        self.preflight_sent.load(Ordering::SeqCst)
    }

    fn set_preflight_sent(&self) {
        self.preflight_sent.store(true, Ordering::SeqCst)
    }

    fn preflight_received(&self) -> bool {
        self.preflight_received.load(Ordering::SeqCst)
    }

    fn set_preflight_received(&self) {
        self.preflight_received.store(true, Ordering::SeqCst);
    }

    fn increment_send_message_count(&self) {
        self.send_message_count.fetch_add(1, Ordering::SeqCst);
    }

    fn increment_send_bytes(&self, len: u64) {
        self.send_bytes.fetch_add(len, Ordering::SeqCst);
    }

    fn increment_recv_message_count(&self) {
        self.recv_message_count.fetch_add(1, Ordering::SeqCst);
    }

    fn increment_recv_bytes(&self, len: u64) {
        self.recv_bytes.fetch_add(len, Ordering::SeqCst);
    }
}

async fn read_preflight_frame_from_stream(
    recv_stream: &DynIrohRecvStream,
    max_frame_bytes: usize,
) -> K2Result<(Url, Bytes)> {
    let mut header_bytes = [0u8; FRAME_HEADER_LEN];
    recv_stream
        .read_exact(&mut header_bytes)
        .await
        .map_err(|err| {
            K2Error::other_src("preflight header read failed", err)
        })?;
    let (frame_type, data_len) =
        decode_frame_header(&header_bytes, max_frame_bytes)?;
    debug!(?frame_type, ?data_len, "decoded preflight frame header");
    if frame_type == FrameType::Data {
        return Err(K2Error::other(
            "preflight frame expected, received data frame",
        ));
    };
    let mut preflight_bytes = vec![0u8; data_len];
    recv_stream
        .read_exact(&mut preflight_bytes)
        .await
        .map_err(|err| K2Error::other_src("preflight data read failed", err))?;
    let (remote_url, preflight_bytes) =
        decode_frame_preflight(&preflight_bytes)?;
    debug!(remote = ?remote_url.peer_id(), "decoded preflight frame data");
    Ok((remote_url, preflight_bytes))
}

async fn read_data_frame_from_stream(
    recv_stream: &DynIrohRecvStream,
    max_frame_bytes: usize,
) -> K2Result<(Vec<u8>, usize)> {
    // Read data frame header
    let mut header = [0u8; FRAME_HEADER_LEN];
    recv_stream.read_exact(&mut header).await.map_err(|err| {
        K2Error::other_src("error reading data frame header", err)
    })?;
    let (frame_type, data_len) = decode_frame_header(&header, max_frame_bytes)
        .map_err(|err| {
            K2Error::other_src("failed to decode iroh frame header", err)
        })?;
    if frame_type == FrameType::Preflight {
        return Err(K2Error::other(
            "data frame expected, received preflight frame",
        ));
    }
    // Read data frame data
    let mut data = vec![0u8; data_len];
    recv_stream.read_exact(&mut data).await.map_err(|err| {
        K2Error::other_src("error reading data frame data", err)
    })?;
    trace!(?data, "incoming data frame");
    Ok((data, data_len))
}
