# vpp-oxidize

Writing [VPP](https://fd.io/) plugins in Rust — an incremental path to
oxidizing VPP, starting where the seam already exists: the plugin ABI.

An **unmodified** VPP binary dlopens a Rust `cdylib`, registers its graph
node, feature-arc entry and CLI commands through the exact same
constructor mechanism the C macros use, and forwards packets through
Rust dataplane code.

```
$ vppctl show plugins | grep rateguard
 59. rateguard_plugin.so   0.1.0   Per-source-IPv4 token bucket rate limiter (Rust)

$ vppctl show trace
  ...
  00:01:00:840825: rateguard
    rateguard: src 10.0.0.2 PASS (9.00 tokens left)
```

## Layout

| crate | role |
|---|---|
| `vpp-sys` | Raw bindgen FFI over `vlib`/`vnet`/`vppinfra` headers. Static-inline functions are exposed via generated C shims (`--wrap-static-fns`). Layout is verified against C at compile time by bindgen's generated asserts. |
| `vpp` | Safe(ish) wrappers. **All `unsafe` plumbing lives here**: plugin registration (`.vlib_plugin_registration` ELF section), node/feature/CLI registration via `.init_array` constructors (mirroring `VLIB_REGISTER_NODE` & friends), `NextFrames` (the get/validate/put next-frame protocol), `Buffer`, trace helpers. |
| `plugins/rateguard` | A real plugin: per-source-IPv4 token-bucket rate limiter as an `ip4-unicast` feature, with CLI, error counters and packet tracing. |

## Build & test

Prereqs: a built VPP tree (default: sibling `../vpp` with
`build-root/install-oxidize/vpp` or `install-vpp_debug-native/vpp`;
override with `VPP_PREFIX`), Rust 1.85+, libclang.

```
make plugins          # cargo build + stage target/plugins/rateguard_plugin.so
make test             # unit/layout tests
./scripts/e2e-test.sh # boots real VPP, injects 100-pkt burst, asserts 10/90
```

Run interactively:

```
make run-vpp
vpp# set rateguard rate 100 burst 10
vpp# rateguard interface pg0
vpp# show rateguard
```

## How registration works

VPP's C registration macros (`VLIB_REGISTER_NODE`, `VNET_FEATURE_INIT`,
`VLIB_CLI_COMMAND`) expand to ELF constructors that link a static struct
into lists hanging off exported globals (`vlib_global_main`,
`feature_main`) at `dlopen()` time — before VPP's init processes those
lists. The `vpp` crate does literally the same from Rust: zeroed statics
filled and linked by `#[link_section = ".init_array"]` constructors
(`vpp::ctor!`). The plugin loader's `vlib_plugin_registration` struct is
the one thing that must be a compile-time constant (the loader reads the
ELF section from disk before dlopen), so it has a const-constructible
mirror type, layout-asserted against the bindgen struct.

## Notes / caveats

- Build with the same `-march` as the VPP being targeted (`VPP_MARCH`,
  default `x86-64-v2`) — some VPP structs contain SIMD-typed members.
- Like C plugins, the binary is tied to the exact VPP version it was
  built against; bindgen regenerates per build, so this is enforced
  naturally at load time (struct layout checks are compile-time).
- Static-inline shims cost a function call. Fine for control paths and
  for bring-up; hot per-packet accessors should graduate to native Rust
  reimplementations against the (asserted) struct layouts.
- `panic = "abort"` everywhere: a Rust panic must not unwind into C.

## Roadmap

- [ ] `#[vpp_node]` proc-macro to shrink node boilerplate
- [ ] Reimplement hot inline accessors (buffer, feature-next) in Rust
- [ ] vec/pool typed views (`VppVec<T>`) over vppinfra layouts
- [ ] Binary API (.api) handler generation for Rust plugins
- [ ] Multi-arch node variants (`VLIB_NODE_FN` equivalent)
- [ ] CI matrix against VPP master
