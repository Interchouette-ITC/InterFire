fn main() {
    println!("cargo:rerun-if-changed=bpf/interfire-ebpf-programs");
}
