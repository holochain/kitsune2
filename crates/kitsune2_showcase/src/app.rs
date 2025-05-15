use bytes::Bytes;
use chrono::{DateTime, Local};
use kitsune2_api::*;
use kitsune2_core::{factories::MemoryOp, get_all_remote_agents};
use kitsune2_transport_tx5::{IceServers, WebRtcConfig};
use std::{fmt::Debug, sync::Arc, time::SystemTime};
use tokio::sync::mpsc;

use crate::Args;

// hard-coded random space
const DEF_SPACE: SpaceId = SpaceId(Id(Bytes::from_static(&[
    215, 33, 182, 196, 173, 34, 116, 214, 251, 139, 163, 71, 112, 51, 234, 167,
    61, 62, 237, 27, 79, 162, 114, 232, 16, 184, 183, 235, 147, 138, 247, 202,
])));

pub struct App {
    _kitsune: DynKitsune,
    transport: DynTransport,
    space: DynSpace,
    _agent: Arc<kitsune2_core::Ed25519LocalAgent>,
    printer_tx: mpsc::Sender<String>,
}

impl App {
    pub async fn new(
        printer_tx: mpsc::Sender<String>,
        args: Args,
    ) -> K2Result<Self> {
        let space = if let Some(seed) = args.network_seed {
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
                _space: SpaceId,
                data: Bytes,
            ) -> K2Result<()> {
                let printer_tx = self.0.clone();
                tokio::task::spawn(async move {
                    printer_tx
                        .send(String::from_utf8_lossy(&data).into())
                        .await
                        .unwrap();
                });
                Ok(())
            }
        }

        #[derive(Debug)]
        struct K(mpsc::Sender<String>);

        impl KitsuneHandler for K {
            fn create_space(
                &self,
                _space: SpaceId,
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
                        .unwrap();
                })
            }
        }

        let builder = kitsune2::default_builder().with_default_config()?;

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

        let h: DynKitsuneHandler = Arc::new(K(printer_tx.clone()));
        let kitsune = builder.build().await?;
        kitsune.register_handler(h).await?;
        let transport = kitsune.transport().await?;
        let space = kitsune.space(space).await?;

        let agent = Arc::new(kitsune2_core::Ed25519LocalAgent::default());

        space.local_agent_join(agent.clone()).await?;

        Ok(Self {
            _kitsune: kitsune,
            transport,
            space,
            _agent: agent,
            printer_tx,
        })
    }

    pub async fn stats(&self) -> K2Result<()> {
        let stats = self.transport.dump_network_stats().await?;
        self.printer_tx.send(format!("{stats:#?}")).await.unwrap();
        Ok(())
    }

    pub async fn chat(&self, msg: Bytes) -> K2Result<()> {
        // this is a very naive chat impl, just sending to peers we know about

        self.printer_tx
            .send("checking for peers to chat with...".into())
            .await
            .unwrap();
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
            .unwrap();

        for peer in peers {
            let printer_tx = self.printer_tx.clone();
            let space = self.space.clone();
            let msg = msg.clone();
            tokio::task::spawn(async move {
                match space.send_notify(peer.url.clone().unwrap(), msg).await {
                    Ok(_) => {
                        printer_tx
                            .send(format!("chat to {} success", &peer.agent))
                            .await
                    }
                    Err(err) => {
                        printer_tx
                            .send(format!(
                                "chat to {} failed: {err:?}",
                                &peer.agent
                            ))
                            .await
                    }
                }
            });
        }

        Ok(())
    }

    pub async fn share(&self, data: String) -> K2Result<()> {
        self.printer_tx
            .send(format!("Sending data '{data}'"))
            .await
            .unwrap();

        let op = MemoryOp::new(Timestamp::now(), data.into());
        let op_id = op.compute_op_id();

        self.space
            .op_store()
            .process_incoming_ops(vec![op.clone().into()])
            .await
            .unwrap();

        self.printer_tx
            .send(format!("Op '{op_id}' successfully stored"))
            .await
            .unwrap();

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
            .await
            .unwrap()
            .0;

        let stored_ops = self
            .space
            .op_store()
            .retrieve_ops(op_ids.to_vec())
            .await
            .unwrap();

        if !stored_ops.is_empty() {
            self.printer_tx
                .send("ID\t\t\t\t\t\tCREATED AT".to_string())
                .await
                .unwrap();

            for op in stored_ops {
                let mem_op = MemoryOp::from(op.op_data);
                let created_at = DateTime::<Local>::from(SystemTime::from(
                    mem_op.created_at,
                ));
                self.printer_tx
                    .send(format!("{}\t{:?}", op.op_id, created_at))
                    .await
                    .unwrap();
            }
        } else {
            self.printer_tx
                .send("No ops found".to_string())
                .await
                .unwrap();
        }

        Ok(())
    }
}
