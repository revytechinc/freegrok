# Grok Build — source install (no ports/pkg required)
#
# FreeBSD:
#   make build && make doctor
#   doas make install                 # /usr/local/bin/grok-build
#   make install-user                 # $HOME/.local/bin/grok-build (no root)
#
# Always runs cargo for build (incremental; never leaves a stale binary).

.POSIX:

PREFIX ?= /usr/local
DESTDIR ?=
BINDIR ?= $(PREFIX)/bin
DOCDIR ?= $(PREFIX)/share/doc/grok-build
BASHCOMPDIR ?= $(PREFIX)/etc/bash_completion.d
ZSHCOMPDIR ?= $(PREFIX)/share/zsh/site-functions
FISHCOMPDIR ?= $(PREFIX)/share/fish/vendor_completions.d

# FreeBSD native name — coexists with linuxulator `grok`
BINNAME ?= grok-build

CARGO ?= cargo
CARGO_BIN_PKG = xai-grok-pager-bin
CARGO_BIN_NAME = xai-grok-pager
RELEASE_BIN = target/release/$(CARGO_BIN_NAME)

PROTOC ?= protoc

.PHONY: all build release install install-user uninstall doctor check clean help \
	install-bin install-completions install-docs preflight

all: release

help:
	@echo "Grok Build source Makefile (TUI agent — not a system installer)"
	@echo ""
	@echo "  make build         release build (cargo, incremental)"
	@echo "  make doctor        offline gate: doctor --ci"
	@echo "  make install       install $(BINNAME) → $(DESTDIR)$(BINDIR)/  (needs write access)"
	@echo "  make install-user  install → $$HOME/.local (no root)"
	@echo "  make uninstall     remove installed files"
	@echo "  make check         cargo check"
	@echo "  make clean         cargo clean"
	@echo ""
	@echo "PREFIX=$(PREFIX)  BINNAME=$(BINNAME)  PROTOC=$(PROTOC)"
	@echo "Deps: rust/cargo, protoc (pkg install rust protobuf ripgrep)"

preflight:
	@if ! command -v $(CARGO) >/dev/null 2>&1; then \
		echo "error: cargo not found — pkg install rust  (or rustup)"; exit 1; \
	fi
	@if ! command -v "$(PROTOC)" >/dev/null 2>&1 && [ ! -x "$(PROTOC)" ]; then \
		echo "error: protoc not found — pkg install protobuf  (or PROTOC=...)"; exit 1; \
	fi
	@if ! command -v rg >/dev/null 2>&1; then \
		echo "warning: rg missing — runtime needs: pkg install ripgrep"; \
	fi

# Always invoke cargo so source changes (doctor/jail) are not left out of install.
build release: preflight
	env PROTOC="$(PROTOC)" $(CARGO) build -p $(CARGO_BIN_PKG) --release
	@test -x $(RELEASE_BIN) || (echo "error: missing $(RELEASE_BIN)"; exit 1)

check: preflight
	env PROTOC="$(PROTOC)" $(CARGO) check -p $(CARGO_BIN_PKG)

# Offline product gate — critical only, ≤15s (see user-guide 25-doctor.md)
doctor: build
	./$(RELEASE_BIN) doctor --ci

install: build install-bin install-completions install-docs
	@echo ""
	@echo "Installed: $(DESTDIR)$(BINDIR)/$(BINNAME)"
	@echo "Check:     $(BINNAME) doctor --ci"
	@echo "Optional:  $(BINNAME) jail status   # FreeBSD isolation (optional)"
	@echo "Ensure $(BINDIR) is on your PATH."

# No root: ~/.local/bin
install-user:
	@$(MAKE) install PREFIX="$${HOME}/.local"

install-bin: build
	mkdir -p "$(DESTDIR)$(BINDIR)"
	install -m 755 "$(RELEASE_BIN)" "$(DESTDIR)$(BINDIR)/$(BINNAME)"

install-completions: build
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
	-install -m 644 \
		crates/codegen/xai-grok-pager/docs/user-guide/18-sandbox.md \
		"$(DESTDIR)$(DOCDIR)/sandbox.md" 2>/dev/null || true

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/$(BINNAME)"
	rm -f "$(DESTDIR)$(BASHCOMPDIR)/$(BINNAME)"
	rm -f "$(DESTDIR)$(ZSHCOMPDIR)/_$(BINNAME)"
	rm -f "$(DESTDIR)$(FISHCOMPDIR)/$(BINNAME).fish"
	rm -rf "$(DESTDIR)$(DOCDIR)"

clean:
	$(CARGO) clean
