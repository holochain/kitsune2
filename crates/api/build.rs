fn main() -> std::io::Result<()> {
    std::env::set_var("PROTOC", protobuf_src::protoc());
    prost_build::Config::new()
        .bytes(["."])
        .compile_protos(&["proto/wire.proto"], &["proto/"])?;
    Ok(())
}
