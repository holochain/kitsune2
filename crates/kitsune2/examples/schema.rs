use kitsune2_core::factories::{
    CoreBootstrapModConfig, CoreFetchModConfig, CorePublishModConfig,
    CoreSpaceModConfig, MemBootstrapModConfig, MemPeerStoreModConfig,
};
use kitsune2_gossip::K2GossipModConfig;
use kitsune2_transport_tx5::Tx5TransportModConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
struct K2Config {
    #[serde(flatten)]
    core_bootstrap: Option<CoreBootstrapModConfig>,
    #[serde(flatten)]
    core_fetch: Option<CoreFetchModConfig>,
    #[serde(flatten)]
    core_publish: Option<CorePublishModConfig>,
    #[serde(flatten)]
    core_space: Option<CoreSpaceModConfig>,
    #[serde(flatten)]
    mem_bootstrap: Option<MemBootstrapModConfig>,
    #[serde(flatten)]
    mem_peer_store: Option<MemPeerStoreModConfig>,
    #[serde(flatten)]
    k2_gossip: Option<K2GossipModConfig>,
    #[serde(flatten)]
    tx5_transport: Option<Tx5TransportModConfig>,
}

const CURRENT_VALUE: &str = r#"
{
    "coreBootstrap": {
        "backoffMaxMs": 1000,
        "backoffMinMs": 100,
        "serverUrl": "https://test.com"
    },
    "coreFetch": {
        "firstBackOffIntervalMs": 100,
        "lastBackOffIntervalMs": 1000,
        "numBackOffIntervals": 3,
        "parallelRequestCount": 2,
        "reInsertOutgoingRequestDelayMs": 10
    },
    "corePublish": {},
    "coreSpace": {
        "reSignExpireTimeMs": 15000,
        "reSignFreqMs": 120000
    },
    "memBootstrap": {
        "pollFreqMs": 100,
        "testId": "hello-test"
    },
    "memPeerStore": {
        "pruneIntervalS": 10
    },
    "k2Gossip": {
        "initiateJitterMs": 10,
        "initialInitiateIntervalMs": 100,
        "initiateIntervalMs": 100,
        "maxConcurrentAcceptedRounds": 20,
        "maxGossipOpBytes": 1000,
        "maxRequestGossipOpBytes": 1000,
        "minInitiateIntervalMs": 100,
        "roundTimeoutMs": 50
    },
    "tx5Transport": {
        "serverUrl": "wss://test.com",
        "signalAllowPlainText": false,
        "timeoutS": 10,
        "webrtcConfig": {
            "bundlePolicy": "max-bundle",
            "iceCandidatePoolSize": 30,
            "iceServers": [
                {
                    "urls": [
                        "stun:a.stun.server",
                        "stun:b.stun.server"
                    ]
                }
            ],
            "rtcpMuxPolicy": "negotiate"
        }
    },
    "extensionModule": {
        "someField": "someValue"
    }
}
"#;

fn main() {
    let schema = schemars::schema_for!(K2Config);

    let validator = jsonschema::validator_for(
        &serde_json::to_value(&schema).expect("Could not serialize schema"),
    )
    .expect("Could not create schema validator");

    match validator.validate(
        &serde_json::from_str(CURRENT_VALUE)
            .expect("Current value is not valid JSON"),
    ) {
        Ok(_) => println!("Current value is valid against the schema"),
        Err(error) => {
            eprintln!("Current value is not valid against the schema");
            eprintln!("{}", error);
        }
    }
}
