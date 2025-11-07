fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only compile etcd protobuf files when the etcd feature is enabled
    #[cfg(feature = "etcd")]
    {
        // Use protox (pure Rust) instead of requiring protoc binary
        // This compiles .proto files without needing the system protoc binary
        let file_descriptor_set = protox::compile(
            ["proto/etcd/rpc.proto"],
            ["proto/etcd"],
        )?;

        // Use tonic-build with the file descriptor set
        tonic_build::configure()
            .build_server(true)
            .build_client(false)
            .compile_fds(file_descriptor_set)?;
    }

    Ok(())
}
