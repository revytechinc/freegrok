# Grok Build — source install (no ports required)
#
# Defaults:
#   make            → build release binary (same as make build / make release)
#   make install    → build first, require binary present, then install
#
# One install interface:
#   make && make doctor && make install
#
# install tries PREFIX (default /usr/local). If that tree is not writable,
# the *same* install runs under $$HOME/.local — same BINNAME, same paths
# relative to PREFIX. No separate install-user target.
#
# DESTDIR= staging never falls back (packaging must use an explicit PREFIX).

.POSIX:

PREFIX ?= /usr/local
DESTDIR ?=
BINNAME ?= grok-build

CARGO ?= cargo
CARGO_BIN_PKG = xai-grok-pager-bin
CARGO_BIN_NAME = xai-grok-pager
RELEASE_BIN = target/release/$(CARGO_BIN_NAME)
PROTOC ?= protoc

# Resolved install prefix (set by install recipe; used by _install_*)
IPREFIX = $(PREFIX)

BINDIR = $(IPREFIX)/bin
DOCDIR = $(IPREFIX)/share/doc/grok-build
SHAREDIR = $(IPREFIX)/share/grok-build
BASHCOMPDIR = $(IPREFIX)/etc/bash_completion.d
ZSHCOMPDIR = $(IPREFIX)/share/zsh/site-functions
FISHCOMPDIR = $(IPREFIX)/share/fish/vendor_completions.d
MODELS_DEV_GZ = crates/codegen/xai-grok-models/catalog/models_dev.json.gz

.PHONY: all build release install uninstall doctor check clean help preflight \
	require-build ensure-build _install_all _install_bin _install_completions \
	_install_docs _install_catalog

# Default goal: bare "make" builds the release binary.
all: build

help:
	@echo "Grok Build source Makefile (TUI agent)"
	@echo ""
	@echo "  make            release binary (default; same as make build)"
	@echo "  make build      release binary (cargo, incremental)"
	@echo "  make doctor     offline gate: doctor --ci (builds if binary missing)"
	@echo "  make install    install $(BINNAME) (builds if needed; PREFIX=$(PREFIX); falls back to ~/\.local)"
	@echo "  make uninstall  remove from PREFIX (or set PREFIX=$$HOME/.local)"
	@echo "  make check      cargo check"
	@echo "  make clean      cargo clean"
	@echo ""
	@echo "PREFIX=$(PREFIX)  BINNAME=$(BINNAME)  PROTOC=$(PROTOC)"
	@echo "Deps: pkg install rust protobuf ripgrep"
	@echo "Example: make && make install  # build, then install"

preflight:
	@if ! command -v $(CARGO) >/dev/null 2>&1; then \
		echo "error: cargo not found — pkg install rust  (or rustup)"; exit 1; \
	fi
	@if ! command -v "$(PROTOC)" >/dev/null 2>&1 && [ ! -x "$(PROTOC)" ]; then \
		echo "error: protoc not found — pkg install protobuf  (or PROTOC=...)"; exit 1; \
	fi
	@if ! command -v rg >/dev/null 2>&1; then \
		echo "warning: rg missing — runtime: pkg install ripgrep"; \
	fi

build release: preflight
	@echo "==> build $(CARGO_BIN_PKG) --release"
	env PROTOC="$(PROTOC)" $(CARGO) build -p $(CARGO_BIN_PKG) --release
	@test -x $(RELEASE_BIN) || (echo "error: build finished but missing $(RELEASE_BIN)"; exit 1)
	@echo "==> build ok: $(RELEASE_BIN)"

# Fail if release binary is missing (e.g. install after clean without make).
require-build:
	@if [ ! -x $(RELEASE_BIN) ]; then \
		echo "error: $(RELEASE_BIN) missing — run: make  (or make build)"; \
		exit 1; \
	fi

# Build only when the release binary is absent (avoids re-cargo under doas/root).
ensure-build:
	@if [ ! -x $(RELEASE_BIN) ]; then \
		echo "==> $(RELEASE_BIN) missing; building first"; \
		$(MAKE) build; \
	else \
		echo "==> using existing $(RELEASE_BIN)"; \
	fi

check: preflight
	env PROTOC="$(PROTOC)" $(CARGO) check -p $(CARGO_BIN_PKG)

doctor: ensure-build require-build
	./$(RELEASE_BIN) doctor --ci

# Single interface: ensure binary exists (build if needed), then install.
# Prefer: make && doas/sudo make install  so cargo never runs as root.
# PREFIX default /usr/local; if unwritable (and DESTDIR empty) → $$HOME/.local
install: ensure-build require-build
	@dest="$(DESTDIR)"; \
	pref="$(PREFIX)"; \
	if [ -n "$$dest" ]; then \
		echo "==> staged install DESTDIR=$$dest PREFIX=$$pref"; \
		$(MAKE) _install_all IPREFIX="$$pref" DESTDIR="$$dest" BINNAME="$(BINNAME)"; \
	elif mkdir -p "$$pref/bin" 2>/dev/null \
		&& touch "$$pref/bin/.grok-write-test" 2>/dev/null; then \
		rm -f "$$pref/bin/.grok-write-test"; \
		echo "==> install PREFIX=$$pref"; \
		$(MAKE) _install_all IPREFIX="$$pref" DESTDIR="" BINNAME="$(BINNAME)"; \
	else \
		userp="$$HOME/.local"; \
		echo "==> $$pref not writable; installing to $$userp (same layout, same $(BINNAME))"; \
		echo "    (for system-wide: make && doas/sudo make install PREFIX=$$pref)"; \
		$(MAKE) _install_all IPREFIX="$$userp" DESTDIR="" BINNAME="$(BINNAME)"; \
	fi

_install_all: require-build _install_bin _install_completions _install_docs _install_catalog
	@echo ""
	@echo "Installed: $(DESTDIR)$(BINDIR)/$(BINNAME)"
	@echo "Check:     $(BINNAME) doctor --ci"
	@echo "Optional:  $(BINNAME) jail status"
	@echo "PATH:      ensure $(BINDIR) is on PATH"

_install_bin: require-build
	mkdir -p "$(DESTDIR)$(BINDIR)"
	install -m 755 "$(RELEASE_BIN)" "$(DESTDIR)$(BINDIR)/$(BINNAME)"

_install_completions: require-build
	mkdir -p "$(DESTDIR)$(BASHCOMPDIR)" "$(DESTDIR)$(ZSHCOMPDIR)" "$(DESTDIR)$(FISHCOMPDIR)"
	./$(RELEASE_BIN) completions bash | sed "s/grok/$(BINNAME)/g" \
		> "$(DESTDIR)$(BASHCOMPDIR)/$(BINNAME)"
	./$(RELEASE_BIN) completions zsh | sed "s/grok/$(BINNAME)/g" \
		> "$(DESTDIR)$(ZSHCOMPDIR)/_$(BINNAME)"
	./$(RELEASE_BIN) completions fish | sed "s/grok/$(BINNAME)/g" \
		> "$(DESTDIR)$(FISHCOMPDIR)/$(BINNAME).fish"
	chmod 644 \
		"$(DESTDIR)$(BASHCOMPDIR)/$(BINNAME)" \
		"$(DESTDIR)$(ZSHCOMPDIR)/_$(BINNAME)" \
		"$(DESTDIR)$(FISHCOMPDIR)/$(BINNAME).fish"

_install_docs:
	mkdir -p "$(DESTDIR)$(DOCDIR)"
	-install -m 644 README.md "$(DESTDIR)$(DOCDIR)/" 2>/dev/null || true
	-install -m 644 docs/freebsd-port-and-jail-sandbox.md \
		"$(DESTDIR)$(DOCDIR)/" 2>/dev/null || true
	-install -m 644 \
		crates/codegen/xai-grok-pager/docs/user-guide/25-doctor.md \
		"$(DESTDIR)$(DOCDIR)/doctor.md" 2>/dev/null || true
	-install -m 644 \
		crates/codegen/xai-grok-pager/docs/user-guide/18-sandbox.md \
		"$(DESTDIR)$(DOCDIR)/sandbox.md" 2>/dev/null || true

# Offline models.dev snapshot (also embedded in the binary).
_install_catalog:
	mkdir -p "$(DESTDIR)$(SHAREDIR)"
	-install -m 644 "$(MODELS_DEV_GZ)" \
		"$(DESTDIR)$(SHAREDIR)/models_dev.json.gz" 2>/dev/null || true

# Uninstall from PREFIX (default /usr/local). If you fell back to ~/.local:
#   make uninstall PREFIX=$$HOME/.local
uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/$(BINNAME)"
	rm -f "$(DESTDIR)$(PREFIX)/etc/bash_completion.d/$(BINNAME)"
	rm -f "$(DESTDIR)$(PREFIX)/share/zsh/site-functions/_$(BINNAME)"
	rm -f "$(DESTDIR)$(PREFIX)/share/fish/vendor_completions.d/$(BINNAME).fish"
	rm -rf "$(DESTDIR)$(PREFIX)/share/doc/grok-build"
	rm -f "$(DESTDIR)$(PREFIX)/share/grok-build/models_dev.json.gz"
	-rmdir "$(DESTDIR)$(PREFIX)/share/grok-build" 2>/dev/null || true

clean:
	$(CARGO) clean
