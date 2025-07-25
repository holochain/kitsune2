use super::{IncomingAgentInfoEncoded, IncomingPublishOps};
use kitsune2_api::*;
use prost::Message;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub(super) struct PublishMessageHandler {
    pub(super) incoming_publish_ops_tx: Sender<IncomingPublishOps>,
    pub(super) incoming_publish_agent_tx: Sender<IncomingAgentInfoEncoded>, // takes a json encoded AgentInfoSigned
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
                self.incoming_publish_ops_tx
                    .try_send((request.into(), peer))
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
    use bytes::Bytes;
    use kitsune2_api::*;
    use kitsune2_test_utils::id::create_op_id_list;
    use kitsune2_test_utils::space::TEST_SPACE_ID;
    use prost::Message;
    use std::time::Duration;

    #[test]
    fn decoding_error() {
        let (incoming_publish_ops_tx, _) = tokio::sync::mpsc::channel(1);
        let (incoming_publish_agent_tx, _) = tokio::sync::mpsc::channel(1);
        let message_handler = PublishMessageHandler {
            incoming_publish_ops_tx,
            incoming_publish_agent_tx,
        };
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
        let (incoming_publish_ops_tx, _) = tokio::sync::mpsc::channel(1);
        let (incoming_publish_agent_tx, _) = tokio::sync::mpsc::channel(1);
        let message_handler = PublishMessageHandler {
            incoming_publish_ops_tx,
            incoming_publish_agent_tx,
        };
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
        let (incoming_publish_ops_tx, mut incoming_publish_ops_rx) =
            tokio::sync::mpsc::channel(1);
        let (incoming_publish_agent_tx, _) = tokio::sync::mpsc::channel(1);
        let message_handler = PublishMessageHandler {
            incoming_publish_ops_tx,
            incoming_publish_agent_tx,
        };
        let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
        let requested_op_ids = create_op_id_list(1);
        let request_message =
            serialize_request_message(requested_op_ids.clone());

        let task_handle = tokio::task::spawn({
            let peer = peer.clone();
            async move {
                let (op_ids, url) =
                    incoming_publish_ops_rx.recv().await.unwrap();
                assert_eq!(url, peer);
                assert_eq!(op_ids, requested_op_ids);
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
}
