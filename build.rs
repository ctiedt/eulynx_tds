fn main() {
    tonic_build::compile_protos("proto/rasta.proto").unwrap();
}
