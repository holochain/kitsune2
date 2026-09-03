use crate::IrohTransport;
use crate::SpaceRelays;
use crate::close_code::CloseCode;
use crate::connection::DynConnection;
use crate::connection_registry::{
    ConnectionLifecycle, ConnectionResolution, RegistryEntry,
};
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

/// Reason sent when a preferred connection wins simultaneous-open resolution.
pub(super) const SUPERSEDED_CLOSE_REASON: &[u8] =
    b"superseded by preferred connection";

/// Outcome of the connection reader's accept loop.
struct ReaderExit {
    /// Human-readable description of why the loop ended.
    err: String,
    /// Whether the reader failure should mark the peer unresponsive.
    mark_unresponsive: bool,
}

/// Cleanup action for a stopped connection reader.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReaderCleanup {
    /// Reports that the active peer connection failed.
    PeerGone {
        /// Whether to mark the peer unresponsive.
        mark_unresponsive: bool,
    },
    /// Reports an intentional remote close to local handlers.
    Graceful {
        /// The remote's close reason.
        reason: String,
    },
    /// Closes a superseded or inactive connection without notifying handlers.
    Quiet,
}

/// Selects the cleanup action for a stopped reader.
pub(super) fn classify_exit(
    was_active: bool,
    remote_close: Option<(CloseCode, Bytes)>,
    mark_unresponsive: bool,
) -> ReaderCleanup {
    let superseded_by_remote =
        matches!(remote_close, Some((CloseCode::Superseded, _)));

    if !was_active || superseded_by_remote {
        return ReaderCleanup::Quiet;
    }

    if let Some((CloseCode::Graceful, reason)) = remote_close {
        return ReaderCleanup::Graceful {
            reason: String::from_utf8_lossy(&reason).to_string(),
        };
    }

    ReaderCleanup::PeerGone { mark_unresponsive }
}

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
    lifecycle: ConnectionLifecycle,
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
            lifecycle: ConnectionLifecycle::new(),
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

    /// Waits for preflight completion and simultaneous-open resolution.
    ///
    /// Returns how preflight and simultaneous-open resolution completed.
    pub(super) async fn wait_for_resolution(&self) -> ConnectionResolution {
        self.lifecycle.wait_for_resolution().await
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

    /// Whether this connection has resolved as superseded by a preferred
    /// connection to the same peer, as opposed to closed for any other
    /// reason.
    pub(super) fn is_superseded(&self) -> bool {
        self.lifecycle.is_superseded()
    }

    /// Abort the connection reader task, if it is still running.
    ///
    /// Used during transport shutdown, where the reader must stop
    /// immediately without running its exit cleanup.
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
        self.mark_superseded();
        self.connection
            .close(CloseCode::Superseded, SUPERSEDED_CLOSE_REASON);
    }

    /// Close the connection and inform the local handlers.
    ///
    /// The close `code` and `reason` are sent to the remote in the QUIC
    /// application close frame; use [`CloseCode::Graceful`] for an
    /// intentional disconnect so the remote releases the connection
    /// quietly instead of marking this peer unresponsive. Local handlers
    /// are informed via `peer_disconnect` with the same reason.
    pub fn disconnect(&self, code: CloseCode, reason: String) {
        self.mark_closed();
        info!(reason, remote_url = ?self.remote_url(), "Disconnecting from remote");
        self.connection.close(code, reason.as_bytes());
        if let Some(peer) = self.remote_url() {
            self.handler.peer_disconnect(peer, Some(reason));
        }
    }

    // Spawns an asynchronous task that reads incoming uni-directional
    // streams from an iroh connection until it dies, and then runs the
    // cleanup matching why it died.
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
            let exit =
                Self::run_reader_loop(&ctx, &connections, &local_url).await;
            Self::cleanup_after_exit(ctx, &connections, exit).await;
        })
        .abort_handle()
    }

    // Continuously read and handle incoming uni-directional streams from
    // the iroh connection. There is only one stream at a time incoming from
    // a remote. It's read from until the connection is closed or an error
    // occurs.
    //
    // Errors when receiving the preflight frame lead to a break of the loop
    // accepting incoming streams. The preflight must succeed for data frames
    // to be accepted. The connection cannot recover from a failed preflight,
    // and a new connection must be established.
    async fn run_reader_loop(
        ctx: &Arc<Self>,
        connections: &Connections,
        local_url: &Arc<RwLock<Option<Url>>>,
    ) -> ReaderExit {
        // Temporary preflight errors do not mark the peer unresponsive.
        let mut mark_unresponsive = true;

        let err = loop {
            // Main loop to accept incoming unidirectional streams from the remote peer.
            match ctx.connection.accept_uni().await {
                Ok(stream) => {
                    info!(remote_id = ?ctx.connection.remote_id(), "Accepted incoming stream");
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
                        local_url.clone(),
                    )
                    .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            break "superseded by preferred connection"
                                .to_string();
                        }
                        Err(err) => {
                            // Don't mark peer as unresponsive for NoLocalAgentsDuringPreflight
                            // errors - this is a temporary state that will resolve once an
                            // agent joins. It is not a real failure, so log it quietly and
                            // reserve `error!` for genuine preflight failures.
                            if matches!(
                                err,
                                K2Error::NoLocalAgentsDuringPreflight
                            ) {
                                mark_unresponsive = false;
                                debug!(
                                    ?err,
                                    "Stream closed during preflight; no local agents yet"
                                );
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

        ReaderExit {
            err,
            mark_unresponsive,
        }
    }

    // The reader loop has ended: run the cleanup matching why it ended.
    // See [`ReaderCleanup`] for the possible verdicts and their rationale.
    async fn cleanup_after_exit(
        ctx: Arc<Self>,
        connections: &Connections,
        exit: ReaderExit,
    ) {
        let Some(remote_url) = ctx.remote_url() else {
            ctx.mark_closed();
            // Preflight never completed, so no peer URL was learned and the
            // peer was never surfaced to the handler.
            ctx.connection
                .close(CloseCode::Unspecified, exit.err.as_bytes());
            return;
        };

        let was_active = connections.remove_if_current(&remote_url, &ctx);
        if matches!(
            ctx.connection.remote_close_reason(),
            Some((CloseCode::Superseded, _))
        ) {
            ctx.lifecycle.mark_superseded();
        } else {
            ctx.lifecycle.mark_closed();
        }
        let verdict = classify_exit(
            was_active,
            ctx.connection.remote_close_reason(),
            exit.mark_unresponsive,
        );

        match verdict {
            ReaderCleanup::Graceful { reason } => {
                info!(?remote_url, %reason, "Peer disconnected gracefully");
                ctx.disconnect(CloseCode::Graceful, reason);
            }
            ReaderCleanup::PeerGone { mark_unresponsive } => {
                if mark_unresponsive {
                    info!(?remote_url, "Setting peer unresponsive");
                    if let Err(err) = ctx
                        .handler
                        .set_unresponsive(remote_url.clone(), Timestamp::now())
                        .await
                    {
                        warn!(
                            ?err,
                            ?remote_url,
                            "Failed to set peer unresponsive"
                        );
                    }
                } else {
                    info!(
                        ?remote_url,
                        "Skipping set_unresponsive due to temporary error (no local agents)"
                    );
                }
                ctx.disconnect(CloseCode::Unspecified, exit.err);
            }
            ReaderCleanup::Quiet => {
                debug!(?remote_url, reason = %exit.err, "Connection reader stopped without marking peer unresponsive (superseded or not the active connection)");
                // Only claim `Superseded` toward the remote when this
                // connection actually resolved that way (a genuine
                // simultaneous-open loss). Other reasons a non-active reader
                // stops quietly, such as a rejected preflight, must not be
                // reported as superseded: the sender on the other end treats
                // that code as a signal to retry.
                if ctx.is_superseded() {
                    ctx.connection
                        .close(CloseCode::Superseded, SUPERSEDED_CLOSE_REASON);
                } else {
                    ctx.connection
                        .close(CloseCode::Unspecified, exit.err.as_bytes());
                }
            }
        }
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

                // Select the surviving connection before acknowledging the
                // preflight. The dialer treats that acknowledgement as proof
                // that application data can be sent safely on this connection.
                if !connections.register_candidate(&remote_url, &ctx) {
                    debug!(
                        remote = ?remote_url.peer_id(),
                        "Connection superseded by preferred connection to same peer"
                    );
                    return Ok(None);
                }

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

                if !connections.activate(&remote_url, &ctx) {
                    return Ok(None);
                }
                Ok(Some(remote_url))
            })
                .await
                .map_err(|err| {
                    K2Error::other_src("timed out waiting for preflight", err)
                });
            match result {
                Ok(Ok(Some(_))) => {}
                Ok(Ok(None)) => return Ok(false),
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

    pub(super) fn mark_superseded(&self) {
        self.lifecycle.mark_superseded();
    }

    pub(super) fn mark_closed(&self) {
        self.lifecycle.mark_closed();
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

impl RegistryEntry for ConnectionContext {
    fn lifecycle(&self) -> &ConnectionLifecycle {
        &self.lifecycle
    }

    fn is_preferred(&self) -> bool {
        self.is_preferred_connection()
    }

    fn close_superseded(&self) {
        self.connection
            .close(CloseCode::Superseded, SUPERSEDED_CLOSE_REASON);
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
