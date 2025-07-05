use std::process::Command;
use std::env;
use std::path::Path;

fn main() {

    let build_ebpf = |name:&str| {
        //let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        //let dest_path = Path::new(&out_dir).join("obj").join(format!("{name}.o"));
        let out_dir = env::var("OUT_DIR").unwrap();
        let dest_path = Path::new(&out_dir).join(format!("{name}.o"));
        let src_path = Path::new("src/xdp").join(format!("{name}.c"));
        let status = Command::new("clang")
            .arg("-O2")
            .arg("-target")
            .arg("bpf")
            .arg("-c")
            .arg(format!("src/xdp/{name}.c"))
            .arg("-o")
            .arg(&dest_path)
            .status()
            .expect("Failed to compile eBPF program");

        assert!(status.success());
        println!("cargo:rerun-if-changed={}",src_path.to_string_lossy());
        println!("cargo:rerun-if-changed={}",dest_path.to_string_lossy());
    };
    build_ebpf("xdp_pass");
}
