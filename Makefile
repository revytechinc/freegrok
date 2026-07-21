# Grok Build — source install (no ports/pkg required)
#
# FreeBSD (native):
#   gmake build
#   doas gmake install          # or: sudo gmake install
#   grok-build doctor --ci
#
# Other Unix (optional): set BINNAME=grok if you want the product name.
#
# Variables (override on the command line):
#   PREFIX=/usr/local   DESTDIR=   BINNAME=grok-build
#   PROTOC=...          CARGO=cargo

.POSIX:

# Prefer gmake on FreeBSD for any GNU extensions we add later; this file is
# intentionally simple enough for bmake as well.
PREFIX ?= /usr/local
DESTDIR ?=
BINDIR ?= $(PREFIX)/bin
MANDIR ?= $(PREFIX)/share/man/man1
DOCDIR ?= $(PREFIX)/share/doc/grok-build
BASHCOMPDIR ?= $(PREFIX)/etc/bash_completion.d
ZSHCOMPDIR ?= $(PREFIX)/share/zsh/site-functions
FISHCOMPDIR ?= $(PREFIX)/share/fish/vendor_completions.d

# FreeBSD native install name (coexists with linuxulator `grok`).
# Override: gmake install BINNAME=grok
BINNAME ?= grok-build

CARGO ?= cargo
CARGO_BIN_PKG = xai-grok-pager-bin
CARGO_BIN_NAME = xai-grok-pager
RELEASE_BIN = target/release/$(CARGO_BIN_NAME)

# System protoc on FreeBSD; DotSlash bin/protoc is not FreeBSD-ready.
# Override: make build PROTOC=/usr/local/bin/protoc
PROTOC ?= protoc

.PHONY: all build release install uninstall doctor check clean help \
	install-bin install-completions install-docs

all: release

help:
	@echo "Grok Build source Makefile"
	@echo ""
	@echo "  make build       release build → $(RELEASE_BIN)"
	@echo "  make doctor      offline product gate (doctor --ci)"
	@echo "  make install     install $(BINNAME) to $(DESTDIR)$(BINDIR)/"
	@echo "  make uninstall   remove installed files"
	@echo "  make check       cargo check -p $(CARGO_BIN_PKG)"
	@echo "  make clean       cargo clean"
	@echo ""
	@echo "PREFIX=$(PREFIX)  BINNAME=$(BINNAME)  PROTOC=$(PROTOC)"
	@echo "Example: doas make install PREFIX=/usr/local"

build release: $(RELEASE_BIN)

$(RELEASE_BIN):
	@if ! command -v $(CARGO) >/dev/null 2>&1; then \
		echo "error: cargo not found — install lang/rust or rustup"; exit 1; \
	fi
	@if ! command -v "$(PROTOC)" >/dev/null 2>&1 && [ ! -x "$(PROTOC)" ]; then \
		echo "error: protoc not found — pkg install protobuf  (or set PROTOC=)"; exit 1; \
	fi
	@if ! command -v rg >/dev/null 2>&1; then \
		echo "warning: rg not on PATH — runtime needs textproc/ripgrep (pkg install ripgrep)"; \
	fi
	env PROTOC="$(PROTOC)" $(CARGO) build -p $(CARGO_BIN_PKG) --release

check:
	env PROTOC="$(PROTOC)" $(CARGO) check -p $(CARGO_BIN_PKG)

# Product self-test (must not stall; critical-only). See user-guide 25-doctor.md
doctor: $(RELEASE_BIN)
	./$(RELEASE_BIN) doctor --ci

install: install-bin install-completions install-docs
	@echo "Installed $(DESTDIR)$(BINDIR)/$(BINNAME)"
	@echo "Run: $(BINNAME) doctor --ci"

install-bin: $(RELEASE_BIN)
	mkdir -p "$(DESTDIR)$(BINDIR)"
	install -m 755 "$(RELEASE_BIN)" "$(DESTDIR)$(BINDIR)/$(BINNAME)"

# Completions hardcode the public name "grok"; rewrite to BINNAME.
install-completions: $(RELEASE_BIN)
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

install-docs:
	mkdir -p "$(DESTDIR)$(DOCDIR)"
	-install -m 644 README.md "$(DESTDIR)$(DOCDIR)/" 2>/dev/null || true
	-install -m 644 docs/freebsd-port-and-jail-sandbox.md \
		"$(DESTDIR)$(DOCDIR)/" 2>/dev/null || true
	-install -m 644 \
		crates/codegen/xai-grok-pager/docs/user-guide/25-doctor.md \
		"$(DESTDIR)$(DOCDIR)/doctor.md" 2>/dev/null || true

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/$(BINNAME)"
	rm -f "$(DESTDIR)$(BASHCOMPDIR)/$(BINNAME)"
	rm -f "$(DESTDIR)$(ZSHCOMPDIR)/_$(BINNAME)"
	rm -f "$(DESTDIR)$(FISHCOMPDIR)/$(BINNAME).fish"
	rm -rf "$(DESTDIR)$(DOCDIR)"

clean:
	$(CARGO) clean
