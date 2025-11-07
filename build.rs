fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only compile etcd protobuf files when the etcd feature is enabled
    #[cfg(feature = "etcd")]
    {
        // Compile proto files using prost-build
        let mut prost_build = prost_build::Config::new();

        prost_build.compile_protos(
            &["proto/etcd/rpc.proto", "proto/etcd/kv.proto"],
            &["proto/etcd"],
        )?;
    }

    Ok(())
}
