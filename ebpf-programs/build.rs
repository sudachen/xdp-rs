use std::process::Command;
use std::env;
use std::path::Path;

fn main() {
    let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("..").join("xdp-socket").join("xdp-pass.o");

    // Compile the C eBPF program using clang
    let status = Command::new("clang")
        .arg("-O2")
        .arg("-target")
        .arg("bpf")
        .arg("-c")
        .arg("src/xdp-pass.c")
        .arg("-o")
        .arg(&dest_path)
        .status()
        .expect("Failed to compile eBPF program");

    assert!(status.success());

    // Tell cargo to rerun this script if the C file changes
    println!("cargo:rerun-if-changed=xdp.c");
}
