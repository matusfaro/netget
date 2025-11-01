fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only compile etcd protobuf files when the etcd feature is enabled
    #[cfg(feature = "etcd")]
    {
        tonic_build::configure()
            .build_server(true)
            .build_client(false)
            .compile_protos(
                &["proto/etcd/rpc.proto"],
                &["proto/etcd"],
            )?;
    }

    Ok(())
}
