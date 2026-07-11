# Convenience wrapper: build Rust plugins and stage them with VPP-style
# names (the cdylib is built as lib<name>_plugin.so; VPP convention is
# <name>_plugin.so).

VPP_PREFIX ?= $(abspath ../vpp/build-root/install-oxidize/vpp)
LIBDIR := $(VPP_PREFIX)/lib/x86_64-linux-gnu
STAGE := target/plugins

PLUGINS := rateguard

.PHONY: all plugins test run-vpp clean

all: plugins

plugins:
	cargo build $(CARGO_FLAGS)
	mkdir -p $(STAGE)
	@for p in $(PLUGINS); do \
	  cp -v target/debug/lib$${p}_plugin.so $(STAGE)/$${p}_plugin.so; \
	done

test:
	LD_LIBRARY_PATH=$(LIBDIR) cargo test

run-vpp: plugins
	$(VPP_PREFIX)/bin/vpp unix '{ nodaemon interactive }' \
	  plugins '{ path $(LIBDIR)/vpp_plugins:$(abspath $(STAGE)) }'

clean:
	cargo clean
