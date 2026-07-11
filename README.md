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
make test             # everything, including e2e tests against a real VPP
```

## Testing (the seed of a Rust test framework)

The `vpp-test` crate boots real, isolated VPP instances — each with its
own runtime dir, CLI/API sockets and api-segment prefix — so tests run
in parallel under plain `cargo test` (or `cargo nextest run`). Three
kinds of tests coexist, enabling incremental migration off the Python
framework:

- **Pure Rust functional tests** (`plugins/rateguard/tests/e2e.rs`):
  packets are built with [oside](https://github.com/ayourtch/oside)
  layer stacks (`Ether!()/IP!()/UDP!()`) and injected via
  packet-generator; assertions read CLI/counters. The whole rateguard
  suite (3 tests, each with its own VPP instance) runs in ~1.5 s.
- **Perf tests**: `#[ignore]`d by default; they reuse VPP's own
  per-node rdtsc measurement (`show runtime`, parsed into structs by
  `vpp-test::perf`). The rateguard node measures ~106 clocks/pkt at
  256 vectors/call (release plugin, 1M-packet burst, debug VPP).
  Run: `cargo test --release -p rateguard -- --ignored --nocapture`
- **Typed binary-API client** (`vpp-test::api`, built on
  [ayourtch/vpp-api](https://github.com/ayourtch/vpp-api) transport +
  [latest-vpp-api](https://github.com/ayourtch/latest-vpp-api) generated
  types): message ids are negotiated at connect time by name+CRC, so
  compatibility is per-message rather than per-release. The `Api`
  wrapper hides client_index/context/msg-id bookkeeping — tests call
  `api.interface("pg0")`, `api.set_interface_up(idx)`, `api.cli(...)`.
- **Python bridge tests** (`vpp_test::python_test!(name, "test_punt")`):
  wrap existing `make test` modules as cargo tests so the legacy suite
  stays in one runner while ports land. Heavyweight, so they only
  execute with `VPP_PYTHON_TESTS=1`; otherwise they log a skip.

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

- [x] Declarative registration macros (`define_node!` / `define_feature!` / `define_cli!`)
- [x] Native Rust hot-path buffer accessors (debug builds cross-check them
      against the C shims on every packet)
- [x] `vpp-test` harness: parallel-safe real-VPP integration tests in cargo
- [ ] Grow `vpp-test` toward a full `make test` replacement: binary API
      client (typed .api bindings), packet crafting/parsing, perf and
      coverage measurement as first-class outputs
- [ ] `#[vpp_node]` proc-macro (function-to-node, typed next enums)
- [ ] vec/pool typed views (`VppVec<T>`) over vppinfra layouts
- [ ] Binary API (.api) handler generation for Rust plugins
- [ ] Multi-arch node variants (`VLIB_NODE_FN` equivalent)
- [ ] CI matrix against VPP master
