use std::env;
use std::path::PathBuf;

/// Locate a VPP install tree (headers + shared libs).
/// Override with VPP_PREFIX=<path-to>/install-.../vpp
fn find_vpp_prefix() -> PathBuf {
    if let Ok(p) = env::var("VPP_PREFIX") {
        let p = PathBuf::from(p);
        if p.join("include/vlib/vlib.h").exists() {
            return p;
        }
        panic!("VPP_PREFIX={} has no include/vlib/vlib.h", p.display());
    }
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let build_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("vpp/build-root"))
        .expect("cannot derive ../vpp/build-root from CARGO_MANIFEST_DIR");
    for cand in [
        "install-oxidize/vpp",
        "install-vpp_debug-native/vpp",
        "install-vpp-native/vpp",
    ] {
        let p = build_root.join(cand);
        if p.join("include/vlib/vlib.h").exists() {
            return p;
        }
    }
    panic!(
        "no VPP install tree found under {} — build VPP first or set VPP_PREFIX",
        build_root.display()
    );
}

/// Remove bindgen's zero-sized placeholder structs (from C forward
/// declarations) when a real definition with the same name exists.
/// Placeholders look exactly like:
/// ```text
/// #[repr(C)]
/// #[derive(Copy, Clone)]
/// pub struct NAME {
///     _unused: [u8; 0],
/// }
/// ```
fn strip_placeholder_duplicates(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut def_count = std::collections::HashMap::<String, u32>::new();
    let extract = |line: &str| -> Option<String> {
        line.strip_prefix("pub struct ")
            .and_then(|r| r.split_whitespace().next())
            .map(|s| s.trim_end_matches('{').trim().to_string())
    };
    for l in &lines {
        if let Some(n) = extract(l) {
            *def_count.entry(n).or_insert(0) += 1;
        }
    }
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        // match the 5-line placeholder pattern, preceded by attributes
        let is_placeholder = i + 2 < lines.len()
            && extract(lines[i])
                .map(|n| def_count.get(n.as_str()).copied().unwrap_or(0) > 1)
                .unwrap_or(false)
            && lines[i + 1].trim() == "_unused: [u8; 0],"
            && lines[i + 2].trim() == "}";
        if is_placeholder {
            // drop preceding attribute lines (#[repr(C)] / #[derive(...)])
            while let Some(last) = out.last() {
                if last.trim_start().starts_with("#[") {
                    out.pop();
                } else {
                    break;
                }
            }
            i += 3;
        } else {
            out.push(lines[i]);
            i += 1;
        }
    }
    out.join("\n")
}

fn main() {
    let prefix = find_vpp_prefix();
    let include = prefix.join("include");
    let libdir = prefix.join("lib/x86_64-linux-gnu");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let wrapper = manifest.join("wrapper.h");

    // Must match the -march VPP itself was built with, so that any
    // SIMD-typed struct members get identical layout.
    let march = env::var("VPP_MARCH").unwrap_or_else(|_| "x86-64-v2".to_string());

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=shim.c");
    println!("cargo:rerun-if-env-changed=VPP_PREFIX");
    println!("cargo:rerun-if-env-changed=VPP_MARCH");

    let clang_args = [
        format!("-I{}", include.display()),
        format!("-march={}", march),
        "-D_GNU_SOURCE".to_string(),
    ];

    let static_fns_c = out.join("static_fns.c");

    let bindings = bindgen::Builder::default()
        .header(wrapper.to_str().unwrap())
        .clang_args(&clang_args)
        // static inline functions get real extern wrappers in static_fns.c
        .wrap_static_fns(true)
        .wrap_static_fns_path(static_fns_c.to_str().unwrap())
        // ---- functions the wrapper crate uses ----
        // exported (real) functions
        .allowlist_function("vlib_get_next_frame_internal")
        .allowlist_function("vlib_put_next_frame")
        .allowlist_function("vlib_register_node")
        .allowlist_function("vlib_add_trace")
        .allowlist_function("vlib_cli_command_registration_helper")
        .allowlist_function("vlib_cli_output")
        .allowlist_function("vnet_get_main")
        .allowlist_function("vnet_feature_enable_disable")
        .allowlist_function("unformat")
        .allowlist_function("unformat_ip4_address")
        .allowlist_function("unformat_vnet_sw_interface")
        .allowlist_function("format")
        .allowlist_function("format_ip4_address")
        .allowlist_function("format_vnet_sw_if_index_name")
        .allowlist_function("_clib_error_return")
        // static inline functions (each becomes a __extern shim; keep this
        // list tight — every shim links against VPP's exported symbols)
        .allowlist_function("vlib_get_buffer")
        .allowlist_function("vlib_buffer_get_current")
        .allowlist_function("vlib_buffer_advance")
        .allowlist_function("vlib_frame_vector_args")
        .allowlist_function("vlib_time_now")
        .allowlist_function("vlib_get_thread_index")
        .allowlist_function("vlib_node_increment_counter")
        .allowlist_function("vnet_feature_next")
        .allowlist_function("unformat_check_input")
        .allowlist_function("vlib_get_global_main")
        .allowlist_function("vlib_get_main")
        // ---- types ----
        .allowlist_type("vlib_main_t")
        .allowlist_type("vlib_global_main_t")
        .allowlist_type("vlib_node_registration_t")
        .allowlist_type("vlib_node_runtime_t")
        .allowlist_type("vlib_node_type_t")
        .allowlist_type("vlib_frame_t")
        .allowlist_type("vlib_buffer_t")
        .allowlist_type("vlib_error_desc_t")
        .allowlist_type("vlib_plugin_registration_t")
        .allowlist_type("vlib_cli_command_t")
        .allowlist_type("vnet_main_t")
        .allowlist_type("vnet_feature_registration_t")
        .allowlist_type("vnet_feature_main_t")
        .allowlist_type("ip4_header_t")
        .allowlist_type("ethernet_header_t")
        .allowlist_type("unformat_input_t")
        .allowlist_type("clib_error_t")
        // ---- global data ----
        .allowlist_var("vlib_global_main")
        .allowlist_var("feature_main")
        // ---- macro constants ----
        .allowlist_var("VLIB_.*")
        .allowlist_var("VNET_.*")
        // ip6_address_t is a packed union with SIMD members; Rust cannot
        // express that (packed + over-aligned fields), so provide a plain
        // 16-byte packed struct with identical layout instead.
        .blocklist_type("ip6_address_t")
        .raw_line("#[repr(C, packed)]")
        .raw_line("#[derive(Copy, Clone)]")
        .raw_line("pub struct ip6_address_t { pub as_u8: [u8; 16usize] }")
        .derive_debug(false)
        .derive_copy(true)
        .layout_tests(true)
        .generate()
        .expect("bindgen failed");

    // VPP forward-declares some types as `struct X;` while defining them as
    // `typedef struct {...} X`. C keeps tag and typedef namespaces separate,
    // but bindgen emits both under the same Rust name: the real definition
    // plus a zero-sized `_unused: [u8; 0]` placeholder — a name collision.
    // Strip every placeholder whose name also has a real definition.
    let generated = bindings.to_string();
    let deduped = strip_placeholder_duplicates(&generated);
    std::fs::write(out.join("bindings.rs"), deduped).expect("could not write bindings");

    // Compile the generated static-fn wrappers + our va_list shims.
    cc::Build::new()
        .file(&static_fns_c)
        .file(manifest.join("shim.c"))
        .include(&include)
        .include(&manifest) // static_fns.c includes wrapper.h by path
        .flag(format!("-march={}", march))
        .flag("-D_GNU_SOURCE")
        .flag("-fPIC")
        .warnings(false)
        .compile("vpp_sys_shims");

    println!("cargo:rustc-link-search=native={}", libdir.display());
    for lib in ["vlib", "vnet", "vppinfra", "svm", "vlibmemory", "vlibapi"] {
        println!("cargo:rustc-link-lib=dylib={}", lib);
    }
    // Let tests / dev binaries locate the VPP libs at runtime.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", libdir.display());
}
