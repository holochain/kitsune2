fn main() {
    std::env::set_var("OUT_DIR", "../api/proto/gen");
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(
            &[
                "../api/proto/fetch.proto",
                "../api/proto/op_store.proto",
                "../api/proto/publish.proto",
                "../api/proto/wire.proto",
            ],
            &["../api/proto/"],
        )
        .expect("Failed to compile api protobuf protocol files");
    std::env::set_var("OUT_DIR", "../core/proto/gen");
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(&["../core/proto/space.proto"], &["../core/proto/"])
        .expect("Failed to compile core protobuf protocol files");
    std::env::set_var("OUT_DIR", "../gossip/proto/gen");
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(
            &["../gossip/proto/gossip.proto"],
            &["../gossip/proto/"],
        )
        .expect("Failed to compile gossip protobuf protocol files");
}
