fn main() {
    unsafe {
        std::env::set_var("OUT_DIR", "../api/proto/gen");
    }
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(
            &[
                // Wire protocol for sending messages over the K2 transport
                "../api/proto/wire.proto",
                // Module protocols, as the content of wire messages
                "../api/proto/fetch.proto",
                "../api/proto/op_store.proto",
                "../api/proto/publish.proto",
            ],
            &["../api/proto/"],
        )
        .expect("Failed to compile api protobuf protocol files");
    unsafe {
        std::env::set_var("OUT_DIR", "../gossip/proto/gen");
    }
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(
            &["../gossip/proto/gossip.proto"],
            &["../gossip/proto/"],
        )
        .expect("Failed to compile gossip protobuf protocol files");
}
