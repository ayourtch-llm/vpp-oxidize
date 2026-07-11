# HANDOFF — vpp-oxidize

Context document for continuing this work. Written 2026-07-11 at the end
of the first working session. Read this top to bottom before touching
anything; it contains the "why" that the code can't tell you.

## The mission

Andrew (ayourtch@gmail.com, VPP maintainer) wants to **gradually migrate
VPP (../vpp) to Rust** — new plugins first, then safety-critical pieces
of the core, strangler-fig style. Two parallel tracks emerged:

1. **Dataplane track**: write VPP plugins in Rust against an unmodified
   VPP binary (works today — see rateguard).
2. **Test framework track**: incrementally replace VPP's Python
   `make test` framework with Rust (`vpp-test` crate), giving one runner
   for Rust + legacy Python tests, plus first-class perf measurement.

The Rust-for-Linux model is the explicit reference: raw bindings crate →
safe abstractions crate → leaf code in (mostly) safe Rust; all `unsafe`
concentrated in the wrapper layer.

## What exists and works (all verified end-to-end this session)

- **`vpp-sys`** — bindgen over vlib/vnet/vppinfra headers from a VPP
  install tree. Static-inline functions exposed via `--wrap-static-fns`
  C shims compiled by `cc`. Layouts are compile-time asserted (bindgen
  const asserts), so "it compiles" ⇒ struct layouts match C.
- **`vpp`** — the safe(ish) wrapper crate. Registration mirrors VPP's C
  constructor macros exactly: zeroed statics filled and linked into
  `vlib_global_main` / `feature_main` by `.init_array` ctors at dlopen.
  Key pieces: `plugin_register!` (const-constructed mirror struct in the
  `.vlib_plugin_registration` ELF section — must be compile-time data,
  the loader parses the section from disk pre-dlopen), `define_node!` /
  `define_feature!` / `define_cli!`, `NextFrames` (the
  get/validate/put next-frame protocol), `Buffer` (native accessors;
  debug builds cross-check every access against the C shim via
  `debug_assert`), trace helpers (va_list handled by a C shim).
- **`plugins/rateguard`** — per-source-IPv4 token-bucket rate limiter on
  the ip4-unicast feature arc: 3 CLI commands, error counters, packet
  tracing (Rust `format_trace`), per-worker HashMap state, no locks in
  the datapath. **Perf: ~106 clocks/pkt @ 256 vectors/call** (release
  plugin, debug VPP, 1M-pkt burst) vs ~1050 in debug.
- **`vpp-test`** — the test framework seed:
  - `Vpp::start(&["rateguard"])`: isolated instance (own /tmp workdir,
    CLI + API sockets, api-segment prefix, dpdk disabled) → tests run in
    PARALLEL under cargo test/nextest. Full rateguard suite ≈ 1.5 s.
  - `pg` module: packets built with **oside** (`../oside`, Andrew's
    scapy-like crate) — `Ether!()/IP!()/UDP!()` → `fill()` → hex into
    packet-generator streams.
  - `api` module: **typed binary API client** over Andrew's
    `vpp-api-transport` + `latest-vpp-api` (generated types). Msg ids
    negotiated by name+CRC at connect ⇒ per-message version compat.
    The `Api` wrapper hides client_index/context/msg-id bookkeeping —
    this wrapper is the agreed design sketch for a future vpp-api rework.
  - `perf` module: parses `show runtime` into structs (clocks/pkt etc.).
  - `python` module: `python_test!(name, "test_punt")` wraps legacy
    `make test` modules as cargo tests, gated by `VPP_PYTHON_TESTS=1`.

Tests: `make test` (or `LD_LIBRARY_PATH=<vpp libdir> cargo test`). The
perf test is `#[ignore]`d: run with `--release ... -- --ignored --nocapture`.

## Sibling repos (paths relative to ~/vpp/)

| path | what | state |
|---|---|---|
| `vpp/` | VPP source, branch master (~26.10) | builds via `make build`, see gotcha #1 |
| `oside/` | scapy-like packet crate | used as path dep; see gotcha #4 |
| `vpp-api/` | Andrew's API crates monorepo | our fixes landed as `ead8de5`, PUSHED to github.com/ayourtch/vpp-api |
| `latest-vpp-api/` | generated typed API bindings (daily job regenerates) | used as git dep; core CRCs match 26.10 |

This repo's future home: **github.com/ayourtch-llm/vpp-oxidize** (his
LLM-work org). No `origin` remote configured yet; 9 commits on `main`.

## Environment gotchas (will bite you again)

1. **Stray python hijacks VPP builds**: `~/.local/bin/python3.12` is a
   uv-managed interpreter without `ply`. cmake's FindPython3 picks it
   over `/usr/bin/python3` (3.14). Every VPP build needs
   `vpp_cmake_args="-DPython3_EXECUTABLE=/usr/bin/python3" make build`.
2. **VPP trees**: primary = `vpp/build-root/install-vpp_debug-native/vpp`
   (standard make build, DPDK included). A secondary no-root cmake tree
   exists at `install-oxidize/vpp` (from before deps were installed).
   `vpp-sys/build.rs` and `vpp-test` search in that order; `VPP_PREFIX`
   overrides. After changing trees: bindgen reruns automatically
   (rerun-if-env-changed), but stale `target/` links can need
   `cargo clean -p vpp-sys`.
3. **-march must match**: VPP builds with `-march=x86-64-v2`; bindgen
   clang args and the shim cc build use the same (env `VPP_MARCH`).
   SIMD-typed struct members change layout otherwise.
4. **oside dependency pins**: oside has no lockfile and mixes RustCrypto
   generations (`aes 0.8`/`cipher 0.4` pinned; `des`/`cbc`/`cfb-mode`
   at `*`). Our Cargo.lock pins des 0.8.1 / cbc 0.1.2 / cfb-mode 0.8.2.
   A plain `cargo update` will break oside's SNMP code again —
   re-pin (or fix oside upstream, it's Andrew's crate, he's fine with it).
5. **Parallel VPP instances**: need distinct cli socket, socksvr
   socket-name, runtime-dir AND `api-segment { prefix ... }` (else
   /dev/shm collision), plus `plugin dpdk_plugin.so { disable }` (no
   root → rte_eal_init fails → VPP exits). `vpp-test` does all this.
6. **Unix socket paths < ~108 chars** — keep test workdirs in /tmp.
7. **Root access**: none by default; Andrew can provide a root PTY when
   needed (tttt MCP tools; e.g. `pty-2` was a root shell once). System
   deps (libnuma etc.) are installed now.

## Bindgen lessons encoded in vpp-sys/build.rs (don't relearn)

- VPP forward-declares `struct X;` for `typedef struct {...} X` types →
  bindgen emits duplicate names (real + zero-sized placeholder), and
  containers embed the WRONG one (vlib_main_t size was off). Fixed by
  post-processing bindings.rs (`strip_placeholder_duplicates`).
- `ip6_address_t` is a *packed union with SIMD members* — unrepresentable
  in Rust; blocklisted + hand-written 16-byte packed struct.
- Keep the **function allowlist tight**: every allowlisted static inline
  becomes a shim in one .o; any shim referencing a non-exported symbol
  (VPP builds `-fvisibility=hidden`) breaks the plugin link.
- `vlib_buffer_template_t` is at offset 0 of `vlib_buffer_t` → metadata
  access is a pointer cast. Buffer data pointer = struct FAM offset via
  `offset_of!` + `current_data`.

## Key VPP internals used (verified against source this session)

- Plugin loader: scans `*.so`, parses `.vlib_plugin_registration`
  section from DISK (pre-dlopen version check), then dlopens and dlsyms
  `vlib_plugin_registration`. Registration struct = 448 bytes,
  cacheline-aligned, version[64]/version_required[64]/overrides[256]
  char arrays + 3 pointers. Empty version_required ⇒ no version check.
- Node/feature/CLI registration: ctor links into
  `vgm->node_registrations` / `feature_main.next_feature` /
  `vlib_cli_command_registration_helper()` (real exported fn).
- Feature node pattern: next_nodes = ["error-drop"] (slot 0);
  `vnet_feature_next()` supplies the arc continuation (arc init adds
  next slots dynamically). Drops: set `b->error = node->errors[code]`
  and enqueue to slot 0 — error-drop does counter attribution (do NOT
  also increment the counter; that double-counts — bug fixed here once).
- `vlib_get_buffer` = `bm->buffer_mem_start + (bi << 6)`.

## Pending / next steps (rough priority)

1. **Push this repo** to github.com/ayourtch-llm/vpp-oxidize
   (`git remote add origin ... && git push -u origin main`).
2. **vpp-api surface rework** (Andrew explicitly wants this, deferred):
   factor msg_id/client_index/context out of per-message code — lift
   `vpp-test/src/api.rs` patterns into the vpp-api crates. Also:
   `latest-vpp-api` regen from current api.json (daily job exists);
   consider a bindings crate generated from the local tree via
   vpp-api-gen so the workspace is self-sufficient.
3. **Grow vpp-test toward make-test replacement**: more typed API
   helpers as tests need them; port a real Python test (e.g. something
   small like test_punt) as the first migration proof; coverage story
   (llvm-cov for Rust + VPP gcov build).
4. **Perf parity work**: multi-arch node fn variants, quad-loop +
   prefetch in `NextFrames`/node helpers, replace remaining per-packet
   shims (`vnet_feature_next` is still a C shim call).
5. **`#[vpp_node]` proc-macro** (typed next-node enums, less unsafe in
   plugin code), `VppVec<T>`/`Pool<T>` typed views, .api handler
   generation for Rust *plugins* (server side).
6. Fix oside's dependency pins upstream (or vendor the working combo).

## Memory files

`~/.claude/projects/-home-ayourtch-vpp-vpp-oxidize/memory/` has
`vpp-oxidize-project.md` and `vpp-api-crate-redesign.md` — keep them in
sync with reality; they're the cross-session index into this document.
