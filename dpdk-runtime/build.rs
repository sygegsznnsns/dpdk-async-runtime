use std::env;
use std::path::PathBuf;

fn main() {
    let library = pkg_config::Config::new()
        .atleast_version("22.11")
        .probe("libdpdk")
        .expect("Failed to find DPDK via pkg-config. Is DPDK installed?");

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_function("rte_.*")
        .allowlist_type("rte_.*")
        .allowlist_var("RTE_.*")
        .blocklist_type("rte_rdtsc|rte_rdtsc_external|rte_get_tsc_cycles")
        .blocklist_function("rte_rdtsc|rte_rdtsc_external|rte_get_tsc_cycles")
        .blocklist_item("rte_kvargs_.*")
        .opaque_type("rte_ether_addr")
        .opaque_type("rte_ether_hdr")
        .opaque_type("rte_ipv4_hdr")
        .opaque_type("rte_icmp_hdr")
        .opaque_type("rte_arp_hdr")
        .opaque_type("rte_arp_ipv4")
        .opaque_type("rte_l2tpv2_.*")
        .opaque_type("rte_gtp_.*")
        .opaque_type("rte_svx_.*")
        .layout_tests(false)
        .derive_default(true)
        .size_t_is_usize(true);

    for include in &library.include_paths {
        builder = builder.clang_arg(format!("-I{}", include.display()));
    }

    let bindings = builder
        .generate()
        .expect("Failed to generate DPDK bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Failed to write bindings.rs");

    let mut cc_build = cc::Build::new();
    cc_build
        .file("dpdk_helpers.c")
        .warnings(false)
        .opt_level(3)
        .flag_if_supported("-march=native");
    for include in &library.include_paths {
        cc_build.include(include);
    }
    for (key, value) in &library.defines {
        cc_build.define(key, value.as_deref());
    }
    cc_build.compile("dpdk_helpers");

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=dpdk_helpers.c");
}
