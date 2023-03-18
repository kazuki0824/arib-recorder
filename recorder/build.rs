fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .build_transport(false)
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile(&["proto/page.proto"], &["proto"])?;

    Ok(())
}
