fn main() {
    std::env::set_var("OUT_DIR", "../api/proto/gen");
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(&["../api/proto/wire.proto"], &["../api/proto/"])
        .expect("Failed to compile protobuf protocol files");
    std::env::set_var("OUT_DIR", "../core/proto/gen");
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(&["../core/proto/space.proto"], &["../core/proto/"])
        .expect("Failed to compile protobuf protocol files");
}
