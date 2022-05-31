extern crate prost_build;

fn main() {
    prost_build::compile_protos(
        &[
            "anki/proto/anki/generic.proto",
            "anki/proto/anki/links.proto",
            "anki/proto/anki/backend.proto",
            "anki/proto/anki/sync.proto",
        ],
        &["anki/proto/"]).unwrap(); // imports are relative to here
}
