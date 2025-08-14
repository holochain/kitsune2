use super::{IncomingRequest, IncomingResponse};
use kitsune2_api::*;
use prost::Message;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub(super) struct FetchMessageHandler {
    pub(super) incoming_request_tx: Sender<IncomingRequest>,
    pub(super) incoming_response_tx: Sender<IncomingResponse>,
}

impl TxModuleHandler for FetchMessageHandler {
    fn recv_module_msg(
        &self,
        peer: Url,
        _space_id: SpaceId,
        _module: String,
        data: bytes::Bytes,
    ) -> K2Result<()> {
        tracing::trace!("receiving module message from {peer}");
        let fetch = K2FetchMessage::decode(data).map_err(|err| {
            K2Error::other_src(
                format!("could not decode module message from {peer}"),
                err,
            )
        })?;
        match fetch.fetch_message_type() {
            FetchMessageType::Request => {
                let request =
                    FetchRequest::decode(fetch.data).map_err(|err| {
                        K2Error::other_src(
                            format!("could not decode request from {peer}"),
                            err,
                        )
                    })?;

                if let Err(err) = self
                    .incoming_request_tx
                    .try_send((request.into(), peer.clone()))
                {
                    match err {
                        tokio::sync::mpsc::error::TrySendError::Full(_) => {
                            tracing::info!(?err, "could not insert incoming request into queue, dropping it");
                        }
                        tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                            tracing::warn!(
                                ?peer,
                                "incoming request queue is closed, dropping request"
                            );
                            return Err(K2Error::other_src(
                                "incoming request queue is closed",
                                err,
                            ));
                        }
                    }
                }

                Ok(())
            }
            FetchMessageType::Response => {
                let response =
                    FetchResponse::decode(fetch.data).map_err(|err| {
                        K2Error::other_src(
                            format!("could not decode response from {peer}"),
                            err,
                        )
                    })?;

                if let Err(err) =
                    self.incoming_response_tx.try_send(response.ops)
                {
                    match err {
                        tokio::sync::mpsc::error::TrySendError::Full(_) => {
                            tracing::info!(?err, "incoming response queue is full, dropping response");
                        }
                        tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                            tracing::warn!(
                                ?peer,
                                "incoming response queue is closed, dropping response"
                            );
                            return Err(K2Error::other_src(
                                "incoming response queue is closed",
                                err,
                            ));
                        }
                    }
                }

                Ok(())
            }
            unknown_message => Err(K2Error::other(format!(
                "unknown fetch message: {unknown_message:?}"
            ))),
        }
    }
}

impl TxBaseHandler for FetchMessageHandler {}

#[cfg(test)]
mod test {
    use super::FetchMessageHandler;
    use crate::factories::core_fetch::test::test_utils::{
        make_op, random_peer_url,
    };
    use crate::factories::MOD_NAME;
    use bytes::Bytes;
    use kitsune2_api::*;
    use kitsune2_test_utils::id::{create_op_id_list, random_op_id};
    use kitsune2_test_utils::space::TEST_SPACE_ID;
    use prost::Message;
    use std::time::Duration;

    #[test]
    fn decoding_error() {
        let (incoming_request_tx, _) = tokio::sync::mpsc::channel(1);
        let (incoming_response_tx, _) = tokio::sync::mpsc::channel(1);
        let message_handler = FetchMessageHandler {
            incoming_request_tx,
            incoming_response_tx,
        };
        let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
        let wrong_message = Bytes::from_static(b"this is not a fetch message");
        message_handler
            .recv_module_msg(
                peer,
                TEST_SPACE_ID,
                crate::factories::core_fetch::MOD_NAME.to_string(),
                wrong_message,
            )
            .unwrap_err();
    }

    #[test]
    fn invalid_message_type() {
        let (response_tx, _) = tokio::sync::mpsc::channel(1);
        let (response_received_tx, _) = tokio::sync::mpsc::channel(1);
        let message_handler = FetchMessageHandler {
            incoming_request_tx: response_tx,
            incoming_response_tx: response_received_tx,
        };
        let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
        let request_message = K2FetchMessage {
            fetch_message_type: 3,
            data: Bytes::from_static(b"op"),
        }
        .encode_to_vec()
        .into();

        message_handler
            .recv_module_msg(
                peer,
                TEST_SPACE_ID,
                crate::factories::core_fetch::MOD_NAME.to_string(),
                request_message,
            )
            .unwrap_err();
    }

    #[tokio::test]
    async fn request() {
        let (incoming_request_tx, mut incoming_request_rx) =
            tokio::sync::mpsc::channel(1);
        let (incoming_response_tx, _) = tokio::sync::mpsc::channel(1);
        let message_handler = FetchMessageHandler {
            incoming_request_tx,
            incoming_response_tx,
        };
        let peer = Url::from_str("wss://127.0.0.1:1").unwrap();
        let requested_op_ids = create_op_id_list(1);
        let request_message =
            serialize_request_message(requested_op_ids.clone());

        let task_handle = tokio::task::spawn({
            let peer = peer.clone();
            async move {
                let (op_ids, url) = incoming_request_rx.recv().await.unwrap();
                assert_eq!(url, peer);
                assert_eq!(op_ids, requested_op_ids);
            }
        });

        message_handler
            .recv_module_msg(
                peer,
                TEST_SPACE_ID,
                crate::factories::core_fetch::MOD_NAME.to_string(),
                request_message,
            )
            .unwrap();

        tokio::time::timeout(Duration::from_millis(20), task_handle)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn response() {
        let (incoming_request_tx, _) = tokio::sync::mpsc::channel(1);
        let (incoming_response_tx, mut incoming_response_rx) =
            tokio::sync::mpsc::channel(1);
        let message_handler = FetchMessageHandler {
            incoming_request_tx,
            incoming_response_tx,
        };
        let peer = Url::from_str("wss://127.0.0.1:1").unwrap();

        let op = make_op(vec![0]);
        let expected_ops_data = vec![op.into()];
        let request_message =
            serialize_response_message(expected_ops_data.clone());

        let task_handle = tokio::task::spawn(async move {
            let ops = incoming_response_rx
                .recv()
                .await
                .unwrap()
                .into_iter()
                .map(|op| op.data)
                .collect::<Vec<_>>();
            assert_eq!(ops, expected_ops_data);
        });

        message_handler
            .recv_module_msg(
                peer,
                TEST_SPACE_ID,
                crate::factories::core_fetch::MOD_NAME.to_string(),
                request_message,
            )
            .unwrap();

        tokio::time::timeout(Duration::from_millis(20), task_handle)
            .await
            .unwrap()
            .unwrap();
    }

    /// The request or response queues could be filled up by multiple peers sending us messages.
    /// It's not right to punish an individual peer without evidence that they did something
    /// unreasonable. Returning an error here would close the connection and force the peer to
    /// reconnect and resend the request. That's not actually helpful because we just end up with
    /// more requests we can't service.
    #[tokio::test]
    async fn queue_full_not_an_error() {
        let (incoming_request_tx, _rx1) = tokio::sync::mpsc::channel(10);
        let (incoming_response_tx, _rx2) = tokio::sync::mpsc::channel(10);

        let handler = FetchMessageHandler {
            incoming_request_tx,
            incoming_response_tx,
        };

        for _ in 0..15 {
            handler
                .recv_module_msg(
                    random_peer_url(),
                    TEST_SPACE_ID,
                    MOD_NAME.to_string(),
                    serialize_request_message(vec![random_op_id()]),
                )
                .unwrap();
        }

        for _ in 0..15 {
            handler
                .recv_module_msg(
                    random_peer_url(),
                    TEST_SPACE_ID,
                    MOD_NAME.to_string(),
                    serialize_response_message(vec![make_op(vec![0]).into()]),
                )
                .unwrap();
        }
    }

    #[tokio::test]
    async fn closed_channel_returns_err() {
        let (incoming_request_tx, incoming_request_rx) =
            tokio::sync::mpsc::channel(1);
        let (incoming_response_tx, incoming_response_rx) =
            tokio::sync::mpsc::channel(1);

        let handler = FetchMessageHandler {
            incoming_request_tx,
            incoming_response_tx,
        };

        drop(incoming_request_rx); // close receiver
        drop(incoming_response_rx); // close receiver

        let err = handler
            .recv_module_msg(
                random_peer_url(),
                TEST_SPACE_ID,
                MOD_NAME.to_string(),
                serialize_request_message(vec![random_op_id()]),
            )
            .unwrap_err();

        assert!(
            matches!(err, K2Error::Other { ctx, .. } if ctx.contains("incoming request queue is closed"))
        );

        let err = handler
            .recv_module_msg(
                random_peer_url(),
                TEST_SPACE_ID,
                MOD_NAME.to_string(),
                serialize_response_message(vec![make_op(vec![0]).into()]),
            )
            .unwrap_err();

        assert!(
            matches!(err, K2Error::Other { ctx, .. } if ctx.contains("incoming response queue is closed"))
        );
    }
}
