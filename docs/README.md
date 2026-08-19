# FreeGrok documentation (source tree)

User-facing guides live with the pager package and are what ship under `~/.freegrok/docs/user-guide/` (copied from `~/.grok` on first run):

- [User guide index](../crates/codegen/xai-grok-pager/docs/user-guide/README.md)
- [Diagnostics (`freegrok doctor`)](../crates/codegen/xai-grok-pager/docs/user-guide/26-doctor.md)
- [Sandbox Mode](../crates/codegen/xai-grok-pager/docs/user-guide/18-sandbox.md) (includes FreeBSD)
- [FreeGrok rebrand plan](./freegrok-rebrand-plan.md)

## FreeBSD engineering

| Doc | Contents |
|-----|----------|
| [freebsd-port-and-jail-sandbox.md](./freebsd-port-and-jail-sandbox.md) | Port plan, platform verify, jail design (TUI-scoped), doctor CI gate, verification log |

### Source install without ports (FreeBSD)

```sh
pkg install rust protobuf ripgrep
make build && make doctor
make install                 # /usr/local if writable, else ~/.local — same BINNAME
```

See root [`Makefile`](../Makefile).

### After install

```sh
freegrok --version
freegrok doctor --ci
freegrok jail status
freegrok jail setup          # dry-run; optional isolation
```

Regression harness: `scripts/freebsd-regression.sh` (includes `doctor --ci` after release build).
