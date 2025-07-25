use crate::Args;
use bytes::Bytes;
use chrono::{DateTime, Local};
use file_data::FileData;
use file_op_store::{FileOpStoreFactory, FileStoreLookup};
use kitsune2_api::*;
use kitsune2_core::{factories::MemoryOp, get_all_remote_agents};
use kitsune2_transport_tx5::{IceServers, WebRtcConfig};
use std::{ffi::OsStr, fmt::Debug, path::Path, sync::Arc, time::SystemTime};
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
    sync::mpsc,
};

mod file_data;
mod file_op_store;

// hard-coded random space
const DEF_SPACE: SpaceId = SpaceId(Id(Bytes::from_static(&[
    215, 33, 182, 196, 173, 34, 116, 214, 251, 139, 163, 71, 112, 51, 234, 167,
    61, 62, 237, 27, 79, 162, 114, 232, 16, 184, 183, 235, 147, 138, 247, 202,
])));

pub struct App {
    /// Wrapper around the transport layer implementation, used to get network stats
    transport: DynTransport,

    /// The unique DHT space used to communicate with peers
    ///
    /// This [`App`] only uses a single space with an ID derived from the provided network seed or
    /// the hard-coded [`DEF_SPACE`] if no network seed is provided.
    space: DynSpace,

    /// The channel used to send messages to print to the user
    printer_tx: mpsc::Sender<String>,

    /// A lookup to find an operation's [`OpId`] from a file name
    file_store_lookup: FileStoreLookup,
}

impl App {
    pub async fn new(
        printer_tx: mpsc::Sender<String>,
        args: Args,
    ) -> K2Result<Self> {
        let space_id = if let Some(seed) = args.network_seed {
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(seed);
            SpaceId(Id(Bytes::copy_from_slice(&hasher.finalize())))
        } else {
            DEF_SPACE
        };

        #[derive(Debug)]
        struct S(mpsc::Sender<String>);

        impl SpaceHandler for S {
            fn recv_notify(
                &self,
                _from_peer: Url,
                _space_id: SpaceId,
                data: Bytes,
            ) -> K2Result<()> {
                let printer_tx = self.0.clone();
                tokio::task::spawn(async move {
                    printer_tx
                        .send(String::from_utf8_lossy(&data).into())
                        .await
                        .expect("Failed to print message");
                });
                Ok(())
            }
        }

        #[derive(Debug)]
        struct K(mpsc::Sender<String>);

        impl KitsuneHandler for K {
            fn create_space(
                &self,
                _space_id: SpaceId,
            ) -> BoxFut<'_, K2Result<DynSpaceHandler>> {
                Box::pin(async move {
                    let s: DynSpaceHandler = Arc::new(S(self.0.clone()));
                    Ok(s)
                })
            }

            fn new_listening_address(
                &self,
                this_url: Url,
            ) -> BoxFut<'static, ()> {
                let printer_tx = self.0.clone();
                Box::pin(async move {
                    printer_tx
                        .send(format!("Online at: {this_url}"))
                        .await
                        .expect("Failed to print message");
                })
            }
        }

        let mut builder = kitsune2::default_builder().with_default_config()?;

        builder.config.set_module_config(
            &kitsune2_core::factories::CoreBootstrapModConfig {
                core_bootstrap: kitsune2_core::factories::CoreBootstrapConfig {
                    server_url: args.bootstrap_url,
                    ..Default::default()
                },
            },
        )?;

        builder.config.set_module_config(
            &kitsune2_transport_tx5::Tx5TransportModConfig {
                tx5_transport: kitsune2_transport_tx5::Tx5TransportConfig {
                    signal_allow_plain_text: true,
                    server_url: args.signal_url,
                    timeout_s: 10,
                    webrtc_config: WebRtcConfig {
                        ice_servers: vec![IceServers {
                            urls: vec![
                                "stun://stun.l.google.com:19302".to_string()
                            ],
                            username: None,
                            credential: None,
                            credential_type: None,
                        }],
                        ice_transport_policy: Default::default(),
                    },
                    ..Default::default()
                },
            },
        )?;

        let file_store_lookup = FileStoreLookup::default();
        builder.op_store =
            FileOpStoreFactory::create(file_store_lookup.clone());

        let h: DynKitsuneHandler = Arc::new(K(printer_tx.clone()));
        let kitsune = builder.build().await?;
        kitsune.register_handler(h).await?;
        let transport = kitsune.transport().await?;
        let space = kitsune.space(space_id).await?;

        let agent = Arc::new(kitsune2_core::Ed25519LocalAgent::default());
        agent.set_tgt_storage_arc_hint(DhtArc::FULL);

        space.local_agent_join(agent.clone()).await?;

        Ok(Self {
            transport,
            space,
            printer_tx,
            file_store_lookup,
        })
    }

    pub async fn stats(&self) -> K2Result<()> {
        let stats = self.transport.dump_network_stats().await?;
        self.printer_tx
            .send(format!("{stats:#?}"))
            .await
            .expect("Failed to print message");
        Ok(())
    }

    pub async fn chat(&self, msg: Bytes) -> K2Result<()> {
        // this is a very naive chat impl, just sending to peers we know about

        self.printer_tx
            .send("checking for peers to chat with...".into())
            .await
            .expect("Failed to print message");
        let peers = get_all_remote_agents(
            self.space.peer_store().clone(),
            self.space.local_agent_store().clone(),
        )
        .await?
        .into_iter()
        .filter(|p| p.url.is_some())
        .collect::<Vec<_>>();
        self.printer_tx
            .send(format!("sending to {} peers", peers.len()))
            .await
            .expect("Failed to print message");

        for peer in peers {
            if let Some(url) = peer.url.clone() {
                let printer_tx = self.printer_tx.clone();
                let space = self.space.clone();
                let msg = msg.clone();
                tokio::task::spawn(async move {
                    match space.send_notify(url, msg).await {
                        Ok(_) => {
                            printer_tx
                                .send(format!(
                                    "chat to {} success",
                                    &peer.agent
                                ))
                                .await
                                .expect("Failed to print message");
                        }
                        Err(err) => {
                            printer_tx
                                .send(format!(
                                    "chat to {} failed: {err:?}",
                                    &peer.agent
                                ))
                                .await
                                .expect("Failed to print message");
                        }
                    }
                });
            }
        }

        Ok(())
    }

    pub async fn share(&self, file_path: &Path) -> K2Result<()> {
        const MAX_FILE_SIZE: usize = 1024;

        let mut file = File::open(file_path).await.map_err(|err| {
            K2Error::other_src(
                format!("with path: '{}'", file_path.display()),
                err,
            )
        })?;

        let size = file
            .metadata()
            .await
            .map(|m| m.len() as usize)
            .unwrap_or_default();

        if size > MAX_FILE_SIZE {
            return Err(K2Error::other(format!("'{}' is larger than the allowed {MAX_FILE_SIZE} bytes. Actual size {size}", file_path.display())));
        }

        let mut contents = String::with_capacity(size);

        file.read_to_string(&mut contents).await.map_err(|err| {
            K2Error::other_src(
                format!("with path: '{}'", file_path.display()),
                err,
            )
        })?;

        let file_name = file_path.file_name().and_then(OsStr::to_str).ok_or(
            K2Error::other(format!(
                "Invalid file name: {}",
                file_path.display()
            )),
        )?;

        let data = serde_json::to_string(&FileData {
            name: file_name.to_string(),
            contents,
        })
        .map_err(|err| {
            K2Error::other_src(
                format!("with path: '{}'", file_path.display()),
                err,
            )
        })?;

        self.printer_tx
            .send(format!("Storing contents of '{}'", file_path.display()))
            .await
            .expect("Failed to print message");

        let op = MemoryOp::new(Timestamp::now(), data.into_bytes());
        let op_id = op.compute_op_id();

        self.space
            .op_store()
            .process_incoming_ops(vec![op.clone().into()])
            .await?;

        self.printer_tx
            .send(format!("Op '{op_id}' successfully stored"))
            .await
            .expect("Failed to print message");

        let peers = get_all_remote_agents(
            self.space.peer_store().clone(),
            self.space.local_agent_store().clone(),
        )
        .await?
        .into_iter()
        .filter(|p| p.url.is_some())
        .collect::<Vec<_>>();
        self.printer_tx
            .send(format!("Publishing op to {} peers", peers.len()))
            .await
            .expect("Failed to print message");

        for peer in peers {
            let printer_tx = self.printer_tx.clone();
            let space = self.space.clone();
            let op_id = op_id.clone();
            if let Some(url) = peer.url.clone() {
                tokio::task::spawn(async move {
                    match space.publish().publish_ops(vec![op_id], url).await {
                        Ok(_) => printer_tx
                            .send(format!("Published to {}", &peer.agent))
                            .await
                            .expect("Failed to print message"),
                        Err(err) => printer_tx
                            .send(format!(
                                "Failed to publish to {}: {err:?}",
                                &peer.agent
                            ))
                            .await
                            .expect("Failed to print message"),
                    }
                });
            }
        }

        Ok(())
    }

    pub async fn list(&self) -> K2Result<()> {
        let op_ids = self
            .space
            .op_store()
            .retrieve_op_hashes_in_time_slice(
                DhtArc::FULL,
                Timestamp::from_micros(0),
                Timestamp::now(),
            )
            .await?
            .0;

        let stored_ops =
            self.space.op_store().retrieve_ops(op_ids.to_vec()).await?;

        if !stored_ops.is_empty() {
            self.printer_tx
                .send("NAME\t\t\t\t\t\tCREATED AT\t\t\t\tID".to_string())
                .await
                .expect("Failed to print message");

            for op in stored_ops {
                let mem_op = MemoryOp::from(op.op_data);
                let created_at = DateTime::<Local>::from(SystemTime::from(
                    mem_op.created_at,
                ));
                let file_name =
                    serde_json::from_slice::<FileData>(&mem_op.op_data)
                        .map(|f| f.name)
                        .unwrap_or("<CORRUPTED FILE DATA>".to_string());
                self.printer_tx
                    .send(format!(
                        "{:40}\t{:?}\t{}",
                        file_name, created_at, op.op_id
                    ))
                    .await
                    .expect("Failed to print message");
            }
        } else {
            self.printer_tx
                .send("No ops found".to_string())
                .await
                .expect("Failed to print message");
        }

        Ok(())
    }

    pub async fn fetch(&self, file_name: &str) -> K2Result<()> {
        let op_id = self
            .file_store_lookup
            .lock()
            .expect("failed to lock the file_store_lookup")
            .get(file_name)
            .ok_or(K2Error::other("file name not in store"))?
            .clone();

        let stored_ops =
            self.space.op_store().retrieve_ops(vec![op_id]).await?;

        if !stored_ops.is_empty() {
            if let Some(file_data) = stored_ops
                .into_iter()
                .filter_map(|op| {
                    let mem_op = MemoryOp::from(op.op_data);
                    serde_json::from_slice::<FileData>(&mem_op.op_data).ok()
                })
                .find(|file_data| file_data.name == file_name)
            {
                fs::write(file_name, file_data.contents).await.map_err(
                    |err| {
                        K2Error::other_src(
                            format!("file name: '{file_name}'"),
                            err,
                        )
                    },
                )?;

                self.printer_tx
                    .send(format!("Fetched file '{file_name}'"))
                    .await
                    .expect("Failed to print message");
            } else {
                self.printer_tx
                    .send(format!("File with name '{file_name}' not found"))
                    .await
                    .expect("Failed to print message");
            }
        } else {
            self.printer_tx
                .send("No ops found".to_string())
                .await
                .expect("Failed to print message");
        }

        Ok(())
    }
}
