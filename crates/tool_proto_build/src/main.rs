fn main() {
    std::env::set_var("OUT_DIR", "../api/proto/gen");
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(&["../api/proto/wire.proto"], &["../api/proto/"])
        .expect("Failed to compile protobuf protocol files");
}
