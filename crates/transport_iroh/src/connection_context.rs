use crate::{
    decode_frame_header, decode_frame_preflight,
    frame::{encode_frame, Frame},
    Connections, FrameType, FRAME_HEADER_LEN,
};
use bytes::Bytes;
use iroh::endpoint::{Connection, ConnectionType, RecvStream, SendStream};
use kitsune2_api::{K2Error, K2Result, Timestamp, TxImpHnd, Url};
use n0_watcher::Watcher;
use std::{
    fmt,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};
use tokio::{sync::MutexGuard, task::AbortHandle};
use tracing::{debug, error, info, trace, warn};

pub(super) struct ConnectionContext {
    handler: Arc<TxImpHnd>,
    connection: Arc<Connection>,
    connection_reader_abort_handle: Mutex<Option<AbortHandle>>,
    send_stream: tokio::sync::Mutex<Option<SendStream>>,
    remote_url: RwLock<Option<Url>>,
    preflight_sent: AtomicBool,
    preflight_received: AtomicBool,
    send_message_count: AtomicU64,
    send_bytes: AtomicU64,
    recv_message_count: AtomicU64,
    recv_bytes: AtomicU64,
    opened_at_s: u64,
    connection_type_watcher: Mutex<Option<n0_watcher::Direct<ConnectionType>>>,
}

impl fmt::Debug for ConnectionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionContext").finish()
    }
}

impl ConnectionContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        handler: Arc<TxImpHnd>,
        connection: Arc<Connection>,
        remote_url: Option<Url>,
        preflight_sent: bool,
        opened_at_s: u64,
        connection_type_watcher: Option<n0_watcher::Direct<ConnectionType>>,
        connections: Connections,
        local_url: Arc<RwLock<Option<Url>>>,
    ) -> Arc<Self> {
        let ctx = Arc::new(Self {
            handler,
            connection,
            connection_reader_abort_handle: Mutex::new(None),
            send_stream: tokio::sync::Mutex::new(None),
            remote_url: RwLock::new(remote_url),
            preflight_sent: AtomicBool::new(preflight_sent),
            preflight_received: AtomicBool::new(false),
            send_message_count: AtomicU64::new(0),
            send_bytes: AtomicU64::new(0),
            recv_message_count: AtomicU64::new(0),
            recv_bytes: AtomicU64::new(0),
            opened_at_s,
            connection_type_watcher: Mutex::new(connection_type_watcher),
        });

        // Spawn connection reader to listen for incoming connections on the
        // new connection.
        let connection_reader_abort_handle =
            Self::spawn_connection_reader(ctx.clone(), connections, local_url);
        *ctx.connection_reader_abort_handle.lock().expect("poisoned") =
            Some(connection_reader_abort_handle);

        ctx
    }

    pub async fn send_preflight_frame(
        &self,
        url: Url,
        preflight_bytes: Bytes,
    ) -> K2Result<()> {
        let frame =
            encode_frame(Frame::Preflight((url.clone(), preflight_bytes)))?;

        let mut stream_lock = self.ensure_send_stream().await?;
        let stream = stream_lock.as_mut().expect("stream must exist");

        info!(local_url = ?url, "sending preflight frame");
        trace!(?frame, "sending preflight frame");
        if let Err(err) = stream.write_all(&frame).await {
            error!(?err, "failed to send preflight frame");
            *stream_lock = None;
            return Err(K2Error::other_src(
                "failed to send preflight frame",
                err,
            ));
        }

        Ok(())
    }

    pub async fn send_data_frame(&self, data: Bytes) -> K2Result<()> {
        let data_len = data.len() as u64;
        let frame = encode_frame(Frame::Data(data))?;

        let mut stream_lock = self.ensure_send_stream().await?;
        let stream = stream_lock.as_mut().expect("stream must exist");

        trace!(?frame, "sending data frame");
        if let Err(err) = stream.write_all(&frame).await {
            error!(?err, "failed to send data frame");
            *stream_lock = None;
            return Err(K2Error::other_src("failed to send data frame", err));
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

    pub fn get_connection_type(&self) -> ConnectionType {
        let mut lock = self.connection_type_watcher.lock().expect("poisoned");
        match lock.take() {
            Some(mut watcher) => {
                let connection_type = watcher.get();
                *lock = Some(watcher);
                connection_type
            }
            None => ConnectionType::None,
        }
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

    pub fn disconnect(&self, reason: String) {
        info!(reason, remote_url = ?self.remote_url(), "disconnecting from remote");
        self.connection.close(0u8.into(), reason.as_bytes());
        if let Some(peer) = self.remote_url() {
            self.handler.peer_disconnect(peer, Some(reason));
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
            let err = loop {
                // Main loop to accept incoming unidirectional streams from the remote peer.
                match ctx.connection.accept_uni().await {
                    Ok(stream) => {
                        info!(remote_id = ?ctx.connection.remote_id(), "accepted incoming stream");
                        let connections = connections.clone();
                        let local_url = local_url.clone();
                        // Read frames from the stream. If an error is returned, it means the
                        // preflight couldn't be received. The connection must be closed in that
                        // case, because a successful preflight is the prerequisite for establishing
                        // a connection.
                        //
                        // Errors while receiving data frames from the stream indicate a problem
                        // with the stream and lead to closing it. An `Ok` value is returned, so
                        // that the connection is kept open and the next incoming stream is
                        // awaited.
                        if let Err(err) = Self::handle_incoming_stream(
                            ctx.clone(),
                            stream,
                            connections.clone(),
                            local_url,
                        )
                        .await
                        {
                            error!(?err, "stream closed by remote");
                            break err.to_string();
                        }
                    }
                    Err(err) => {
                        error!(?err, "connection closed by remote");
                        break err.to_string();
                    }
                }
            };

            // An error has occurred, either while accepting incoming streams
            // (most likely connection closed) or while reading the preflight
            // from a stream. The protocol can not recover from this error
            // and the connection must be closed. The remote is marked as
            // unresponsive.
            if let Some(remote_url) = ctx.remote_url() {
                connections
                    .write()
                    .expect("poisoned")
                    .remove(&remote_url);
                info!(?remote_url, "setting peer unresponsive");
                if let Err(err) = ctx.handler.set_unresponsive(remote_url.clone(), Timestamp::now()).await{
                    warn!(?err, ?remote_url, "failed to set peer unresponsive");
                }
            }
            ctx.disconnect(err);
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
    async fn handle_incoming_stream(
        ctx: Arc<Self>,
        mut recv_stream: RecvStream,
        connections: Connections,
        local_url: Arc<RwLock<Option<Url>>>,
    ) -> K2Result<()> {
        if !ctx.preflight_received() {
            let result = tokio::time::timeout(Duration::from_secs(10), async {
                let (remote_url, preflight_bytes) = read_preflight_frame_from_stream(&mut recv_stream).await?;

                ctx.set_remote_url(remote_url.clone());
                ctx.handler
                    .recv_data(remote_url.clone(), preflight_bytes)
                    .await?;
                ctx.set_preflight_received();
                info!(remote = ?remote_url.peer_id(),"preflight received successfully");

                // If the preflight has not been sent yet, it must be the first message
                // sent back to the remote.
                if !ctx.preflight_sent() {
                    let maybe_local_url =
                        local_url.read().expect("poisoned").clone();
                    if let Some(local_url) = maybe_local_url {
                        let return_preflight =
                            ctx.handler.peer_connect(remote_url.clone()).await?;
                        ctx.send_preflight_frame(
                            local_url.clone(),
                            return_preflight,
                        )
                        .await?;
                        info!(peer = ?ctx.connection.remote_id(),?local_url, "sent preflight to peer from url");
                        ctx.set_preflight_sent();
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
                    connections
                        .write()
                        .expect("poisoned")
                        .insert(remote_url, ctx.clone());
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
                &mut recv_stream,
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

        Ok(())
    }

    async fn ensure_send_stream(
        &'_ self,
    ) -> K2Result<MutexGuard<'_, Option<SendStream>>> {
        // Atomically open a new stream if none is present.
        let mut stream_lock = self.send_stream.lock().await;
        if stream_lock.is_none() {
            let stream = self.connection.open_uni().await.map_err(|err| {
                K2Error::other_src("failed to open uni-directional stream", err)
            })?;
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
    recv_stream: &mut RecvStream,
) -> K2Result<(Url, Bytes)> {
    let mut header_bytes = [0u8; FRAME_HEADER_LEN];
    recv_stream
        .read_exact(&mut header_bytes)
        .await
        .map_err(|err| {
            K2Error::other_src("preflight header read failed", err)
        })?;
    let (frame_type, data_len) = decode_frame_header(&header_bytes)?;
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
    recv_stream: &mut RecvStream,
) -> K2Result<(Vec<u8>, usize)> {
    // Read data frame header
    let mut header = [0u8; FRAME_HEADER_LEN];
    recv_stream.read_exact(&mut header).await.map_err(|err| {
        K2Error::other_src("error reading data frame header", err)
    })?;
    let (frame_type, data_len) =
        decode_frame_header(&header).map_err(|err| {
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
