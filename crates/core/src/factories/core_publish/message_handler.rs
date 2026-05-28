use super::{IncomingAgentInfoEncoded, IncomingPublishOps};
use kitsune2_api::*;
use prost::Message;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub(super) struct PublishMessageHandler {
    pub(super) incoming_publish_ops_tx: Sender<IncomingPublishOps>,
    pub(super) incoming_publish_agent_tx: Sender<IncomingAgentInfoEncoded>, // takes a json encoded AgentInfoSigned
    pub(super) max_metadata_bytes: u32,
}

impl TxBaseHandler for PublishMessageHandler {}
impl TxModuleHandler for PublishMessageHandler {
    fn recv_module_msg(
        &self,
        peer: Url,
        _space_id: SpaceId,
        _module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        tracing::trace!("receiving module message from {peer}");
        let publish = K2PublishMessage::decode(data).map_err(|err| {
            K2Error::other_src(
                format!("could not decode module message from {peer}"),
                err,
            )
        })?;
        match publish.publish_message_type() {
            PublishMessageType::Ops => {
                let request =
                    PublishOps::decode(publish.data).map_err(|err| {
                        K2Error::other_src(
                            format!("could not decode publish ops from {peer}"),
                            err,
                        )
                    })?;

                // Filter out entries where the metadata exceeds the size cap.
                let max = self.max_metadata_bytes;
                let ops: Vec<PublishOp> =
                    Vec::<PublishOp>::from(request)
                        .into_iter()
                        .filter(|op| {
                            if let Some(meta) = &op.metadata && meta.len() as u32 > max {
                                tracing::warn!(
                                    op_id = ?op.op_id,
                                    metadata_len = meta.len(),
                                    max_metadata_bytes = max,
                                    "dropping incoming publish op entry: metadata exceeds size cap"
                                );
                                return false;
                            }
                            true
                        })
                        .collect();

                self.incoming_publish_ops_tx
                    .try_send((ops, peer))
                    .map_err(|err| {
                        K2Error::other_src(
                            "could not insert incoming publish ops request into queue",
                            err,
                        )
                    })
            }
            PublishMessageType::Agent => {
                let request =
                    PublishAgent::decode(publish.data).map_err(|err| {
                        K2Error::other_src(
                            format!(
                                "could not decode publish agent from {peer}"
                            ),
                            err,
                        )
                    })?;
                self.incoming_publish_agent_tx
                    .try_send(request.agent_info)
                    .map_err(|err| {
                        K2Error::other_src(
                            "could not insert incoming agent publish into queue",
                            err,
                        )
                    })
            }
            unknown_message => Err(K2Error::other(format!(
                "unknown publish message: {unknown_message:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod test {
    use super::PublishMessageHandler;
    use crate::factories::core_publish::IncomingPublishOps;
    use bytes::Bytes;
    use kitsune2_api::*;
    use kitsune2_test_utils::id::create_op_id_list;
    use kitsune2_test_utils::space::TEST_SPACE_ID;
    use prost::Message;
    use std::time::Duration;
    use tokio::sync::mpsc::{self, Receiver};

    fn make_handler(
        max_metadata_bytes: u32,
    ) -> (PublishMessageHandler, Receiver<IncomingPublishOps>) {
        let (incoming_publish_ops_tx, incoming_publish_ops_rx) =
            mpsc::channel(16);
        let (incoming_publish_agent_tx, _) = mpsc::channel(1);
        let handler = PublishMessageHandler {
            incoming_publish_ops_tx,
            incoming_publish_agent_tx,
            max_metadata_bytes,
        };
        (handler, incoming_publish_ops_rx)
    }

    #[test]
    fn decoding_error() {
        let (message_handler, _) = make_handler(1024);
        let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
        let wrong_message =
            Bytes::from_static(b"this is not a publish message");
        message_handler
            .recv_module_msg(
                peer,
                TEST_SPACE_ID,
                crate::factories::core_publish::PUBLISH_MOD_NAME.to_string(),
                wrong_message,
            )
            .unwrap_err();
    }

    #[test]
    fn invalid_message_type() {
        let (message_handler, _) = make_handler(1024);
        let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
        let request_message = K2FetchMessage {
            fetch_message_type: 9,
            data: Bytes::from_static(b"op"),
        }
        .encode_to_vec()
        .into();

        message_handler
            .recv_module_msg(
                peer,
                TEST_SPACE_ID,
                crate::factories::core_publish::PUBLISH_MOD_NAME.to_string(),
                request_message,
            )
            .unwrap_err();
    }

    #[tokio::test]
    async fn publish_ops() {
        let (message_handler, mut incoming_publish_ops_rx) = make_handler(1024);
        let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
        let op_ids = create_op_id_list(1);
        let publish_ops: Vec<PublishOp> = op_ids
            .into_iter()
            .map(|op_id| PublishOp {
                op_id,
                metadata: None,
            })
            .collect();
        let request_message =
            serialize_publish_ops_message(publish_ops.clone());

        let task_handle = tokio::task::spawn({
            let peer = peer.clone();
            async move {
                let (ops, url) = incoming_publish_ops_rx.recv().await.unwrap();
                assert_eq!(url, peer);
                assert_eq!(ops, publish_ops);
            }
        });

        message_handler
            .recv_module_msg(
                peer,
                TEST_SPACE_ID,
                crate::factories::core_publish::PUBLISH_MOD_NAME.to_string(),
                request_message,
            )
            .unwrap();

        tokio::time::timeout(Duration::from_millis(20), task_handle)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn oversized_metadata_entries_are_dropped() {
        let max = 10u32;
        let (message_handler, mut incoming_publish_ops_rx) = make_handler(max);
        let peer = Url::from_str("wss://127.0.0.1:1").unwrap();

        let ok_op = PublishOp {
            op_id: OpId::from(Bytes::from_static(b"op_ok")),
            metadata: Some(Bytes::from_static(b"small")),
        };
        let big_op = PublishOp {
            op_id: OpId::from(Bytes::from_static(b"op_big")),
            metadata: Some(Bytes::from(vec![0u8; (max + 1) as usize])),
        };
        let message =
            serialize_publish_ops_message(vec![ok_op.clone(), big_op]);

        let task_handle = tokio::task::spawn(async move {
            let (ops, _) = incoming_publish_ops_rx.recv().await.unwrap();
            // Only the within-limit op should pass through.
            assert_eq!(ops, vec![ok_op]);
        });

        message_handler
            .recv_module_msg(
                peer,
                TEST_SPACE_ID,
                crate::factories::core_publish::PUBLISH_MOD_NAME.to_string(),
                message,
            )
            .unwrap();

        tokio::time::timeout(Duration::from_millis(20), task_handle)
            .await
            .unwrap()
            .unwrap();
    }
}
