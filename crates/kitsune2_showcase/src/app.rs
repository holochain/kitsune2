use super::*;
use bytes::Bytes;
use kitsune2_api::*;
use std::sync::Arc;

// hard-coded random space
const SPACE: SpaceId = SpaceId(Id(Bytes::from_static(&[
    215, 33, 182, 196, 173, 34, 116, 214, 251, 139, 163, 71, 112, 51, 234, 167,
    61, 62, 237, 27, 79, 162, 114, 232, 16, 184, 183, 235, 147, 138, 247, 202,
])));

pub struct App {
    _k: DynKitsune,
    s: DynSpace,
    a: Arc<kitsune2_core::Ed25519LocalAgent>,
    p: readline::Print,
}

impl App {
    pub async fn new(print: readline::Print, args: Args) -> K2Result<Self> {
        #[derive(Debug)]
        struct S(readline::Print);

        impl SpaceHandler for S {
            fn recv_notify(
                &self,
                _from_peer: Url,
                _space: SpaceId,
                data: Bytes,
            ) -> K2Result<()> {
                let print = self.0.clone();
                tokio::task::spawn(async move {
                    print.print_line(String::from_utf8_lossy(&data).into());
                });
                Ok(())
            }
        }

        #[derive(Debug)]
        struct K(readline::Print);

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
                let print = self.0.clone();
                Box::pin(async move {
                    print.print_line(format!("Online at: {this_url}"));
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
                },
            },
        )?;

        let h: DynKitsuneHandler = Arc::new(K(print.clone()));
        let k = builder.build().await?;
        k.register_handler(h).await?;
        let s = k.space(SPACE).await?;

        let a = Arc::new(kitsune2_core::Ed25519LocalAgent::default());

        s.local_agent_join(a.clone()).await?;

        Ok(Self {
            _k: k,
            s,
            a,
            p: print,
        })
    }

    pub async fn chat(&self, msg: Bytes) -> K2Result<()> {
        // this is a very naive chat impl, just sending to peers we know about

        self.p
            .print_line("checking for peers to chat with...".into());
        let peers = self
            .s
            .peer_store()
            .get_all()
            .await?
            .into_iter()
            .filter(|p| &p.agent != self.a.agent() && p.url.is_some())
            .collect::<Vec<_>>();
        self.p
            .print_line(format!("sending to {} peers", peers.len()));

        for peer in peers {
            let p = self.p.clone();
            let s = self.s.clone();
            let m = msg.clone();
            tokio::task::spawn(async move {
                match s.send_notify(peer.url.clone().unwrap(), m).await {
                    Ok(_) => {
                        p.print_line(format!("chat to {} success", &peer.agent))
                    }
                    Err(err) => p.print_line(format!(
                        "chat to {} failed: {err:?}",
                        &peer.agent
                    )),
                }
            });
        }

        Ok(())
    }
}
