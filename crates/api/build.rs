fn main() {
    let (protoc_bin, _) = protoc_prebuilt::init("27.1")
        .expect("Failed to initialize protoc_prebuilt binary");
    std::env::set_var("PROTOC", protoc_bin);
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(&["proto/wire.proto"], &["proto/"])
        .expect("Failed to compile protobuf protocol files");
}
