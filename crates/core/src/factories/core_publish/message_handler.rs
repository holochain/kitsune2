use super::IncomingPublishOps;
use kitsune2_api::*;
use prost::Message;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub(super) struct PublishMessageHandler {
    pub(super) incoming_publish_ops_tx: Sender<IncomingPublishOps>,
}

impl TxBaseHandler for PublishMessageHandler {}
impl TxModuleHandler for PublishMessageHandler {
    fn recv_module_msg(
        &self,
        peer: Url,
        _space: SpaceId,
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
                            "could not insert incoming request into queue",
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
    // use super::FetchMessageHandler;
    // use crate::factories::core_fetch::test::test_utils::make_op;
    // use bytes::Bytes;
    // use kitsune2_api::*;
    // use kitsune2_test_utils::id::random_op_id;
    // use kitsune2_test_utils::space::TEST_SPACE_ID;
    // use prost::Message;
    // use std::time::Duration;

    // #[test]
    // fn decoding_error() {
    //     let (incoming_request_tx, _) = tokio::sync::mpsc::channel(1);
    //     let (incoming_response_tx, _) = tokio::sync::mpsc::channel(1);
    //     let message_handler = FetchMessageHandler {
    //         incoming_request_tx,
    //         incoming_response_tx,
    //     };
    //     let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
    //     let wrong_message = Bytes::from_static(b"this is not a fetch message");
    //     message_handler
    //         .recv_module_msg(
    //             peer,
    //             TEST_SPACE_ID,
    //             crate::factories::core_fetch::MOD_NAME.to_string(),
    //             wrong_message,
    //         )
    //         .unwrap_err();
    // }

    // #[test]
    // fn invalid_message_type() {
    //     let (response_tx, _) = tokio::sync::mpsc::channel(1);
    //     let (response_received_tx, _) = tokio::sync::mpsc::channel(1);
    //     let message_handler = FetchMessageHandler {
    //         incoming_request_tx: response_tx,
    //         incoming_response_tx: response_received_tx,
    //     };
    //     let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
    //     let request_message = K2FetchMessage {
    //         fetch_message_type: 3,
    //         data: Bytes::from_static(b"op"),
    //     }
    //     .encode_to_vec()
    //     .into();

    //     message_handler
    //         .recv_module_msg(
    //             peer,
    //             TEST_SPACE_ID,
    //             crate::factories::core_fetch::MOD_NAME.to_string(),
    //             request_message,
    //         )
    //         .unwrap_err();
    // }

    // #[tokio::test]
    // async fn request() {
    //     let (incoming_request_tx, mut incoming_request_rx) =
    //         tokio::sync::mpsc::channel(1);
    //     let (incoming_response_tx, _) = tokio::sync::mpsc::channel(1);
    //     let message_handler = FetchMessageHandler {
    //         incoming_request_tx,
    //         incoming_response_tx,
    //     };
    //     let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
    //     let requested_op_ids = vec![random_op_id()];
    //     let request_message =
    //         serialize_request_message(requested_op_ids.clone());

    //     let task_handle = tokio::task::spawn({
    //         let peer = peer.clone();
    //         async move {
    //             let (op_ids, url) = incoming_request_rx.recv().await.unwrap();
    //             assert_eq!(url, peer);
    //             assert_eq!(op_ids, requested_op_ids);
    //         }
    //     });

    //     message_handler
    //         .recv_module_msg(
    //             peer,
    //             TEST_SPACE_ID,
    //             crate::factories::core_fetch::MOD_NAME.to_string(),
    //             request_message,
    //         )
    //         .unwrap();

    //     tokio::time::timeout(Duration::from_millis(20), task_handle)
    //         .await
    //         .unwrap()
    //         .unwrap();
    // }

    // #[tokio::test]
    // async fn response() {
    //     let (incoming_request_tx, _) = tokio::sync::mpsc::channel(1);
    //     let (incoming_response_tx, mut incoming_response_rx) =
    //         tokio::sync::mpsc::channel(1);
    //     let message_handler = FetchMessageHandler {
    //         incoming_request_tx,
    //         incoming_response_tx,
    //     };
    //     let peer = Url::from_str("wss://127.0.0.1:1").unwrap();

    //     let op = make_op(vec![0]);
    //     let expected_ops_data = vec![op.into()];
    //     let request_message =
    //         serialize_response_message(expected_ops_data.clone());

    //     let task_handle = tokio::task::spawn(async move {
    //         let ops = incoming_response_rx
    //             .recv()
    //             .await
    //             .unwrap()
    //             .into_iter()
    //             .map(|op| op.data)
    //             .collect::<Vec<_>>();
    //         assert_eq!(ops, expected_ops_data);
    //     });

    //     message_handler
    //         .recv_module_msg(
    //             peer,
    //             TEST_SPACE_ID,
    //             crate::factories::core_fetch::MOD_NAME.to_string(),
    //             request_message,
    //         )
    //         .unwrap();

    //     tokio::time::timeout(Duration::from_millis(20), task_handle)
    //         .await
    //         .unwrap()
    //         .unwrap();
    // }
}
