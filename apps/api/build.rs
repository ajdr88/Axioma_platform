fn main() {
    // Shared with packages/fuml-runtime's own build.sh — one proto file, two codegen paths
    // (tonic-build here, protoc-gen-grpc-java there). tonic-build (via prost-build) needs a
    // protoc binary and does not vendor one; reuse the same protoc.exe already fetched for the
    // Java side's build.sh rather than fetching a second copy.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let tools_dir = std::path::Path::new(&manifest_dir).join("../../packages/fuml-runtime/tools");
    for candidate in ["protoc.exe", "protoc"] {
        let protoc_path = tools_dir.join(candidate);
        if protoc_path.exists() {
            std::env::set_var("PROTOC", &protoc_path);
            break;
        }
    }
    let proto_path = "../../packages/fuml-runtime/proto/fuml_runtime.proto";
    println!("cargo:rerun-if-changed={proto_path}");
    tonic_build::configure()
        .build_server(false)
        .compile_protos(&[proto_path], &["../../packages/fuml-runtime/proto"])
        .expect("compiling fuml_runtime.proto");

    // docs/IMPLEMENTATION_KICKOFF.md Phase 2 (ADR-011) — the cem-archspace Python sidecar's
    // contract. Same protoc.exe reuse as above; a second, independent proto (own package,
    // `axioma.archspace`), not merged into fuml_runtime.proto.
    let archspace_proto_path = "../../packages/cem-archspace/proto/cem_archspace.proto";
    println!("cargo:rerun-if-changed={archspace_proto_path}");
    tonic_build::configure()
        .build_server(false)
        .compile_protos(
            &[archspace_proto_path],
            &["../../packages/cem-archspace/proto"],
        )
        .expect("compiling cem_archspace.proto");
}
