# Grok Build — source install (no ports required)
#
# One install interface:
#   make build && make doctor && make install
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
BASHCOMPDIR = $(IPREFIX)/etc/bash_completion.d
ZSHCOMPDIR = $(IPREFIX)/share/zsh/site-functions
FISHCOMPDIR = $(IPREFIX)/share/fish/vendor_completions.d

.PHONY: all build release install uninstall doctor check clean help preflight \
	_install_all _install_bin _install_completions _install_docs

all: release

help:
	@echo "Grok Build source Makefile (TUI agent)"
	@echo ""
	@echo "  make build      release binary (cargo, incremental)"
	@echo "  make doctor     offline gate: doctor --ci"
	@echo "  make install    install $(BINNAME) (PREFIX=$(PREFIX); falls back to ~/\.local if not writable)"
	@echo "  make uninstall  remove from PREFIX (or set PREFIX=$$HOME/.local)"
	@echo "  make check      cargo check"
	@echo "  make clean      cargo clean"
	@echo ""
	@echo "PREFIX=$(PREFIX)  BINNAME=$(BINNAME)  PROTOC=$(PROTOC)"
	@echo "Deps: pkg install rust protobuf ripgrep"
	@echo "Example: make install          # prefers /usr/local, else userspace"

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
	env PROTOC="$(PROTOC)" $(CARGO) build -p $(CARGO_BIN_PKG) --release
	@test -x $(RELEASE_BIN) || (echo "error: missing $(RELEASE_BIN)"; exit 1)

check: preflight
	env PROTOC="$(PROTOC)" $(CARGO) check -p $(CARGO_BIN_PKG)

doctor: build
	./$(RELEASE_BIN) doctor --ci

# Single interface: try PREFIX; if unwritable (and DESTDIR empty), same install under $$HOME/.local
install: build
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
		echo "    (for system-wide: doas/sudo make install PREFIX=$$pref)"; \
		$(MAKE) _install_all IPREFIX="$$userp" DESTDIR="" BINNAME="$(BINNAME)"; \
	fi

_install_all: _install_bin _install_completions _install_docs
	@echo ""
	@echo "Installed: $(DESTDIR)$(BINDIR)/$(BINNAME)"
	@echo "Check:     $(BINNAME) doctor --ci"
	@echo "Optional:  $(BINNAME) jail status"
	@echo "PATH:      ensure $(BINDIR) is on PATH"

_install_bin:
	mkdir -p "$(DESTDIR)$(BINDIR)"
	install -m 755 "$(RELEASE_BIN)" "$(DESTDIR)$(BINDIR)/$(BINNAME)"

_install_completions:
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

# Uninstall from PREFIX (default /usr/local). If you fell back to ~/.local:
#   make uninstall PREFIX=$$HOME/.local
uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/$(BINNAME)"
	rm -f "$(DESTDIR)$(PREFIX)/etc/bash_completion.d/$(BINNAME)"
	rm -f "$(DESTDIR)$(PREFIX)/share/zsh/site-functions/_$(BINNAME)"
	rm -f "$(DESTDIR)$(PREFIX)/share/fish/vendor_completions.d/$(BINNAME).fish"
	rm -rf "$(DESTDIR)$(PREFIX)/share/doc/grok-build"

clean:
	$(CARGO) clean
