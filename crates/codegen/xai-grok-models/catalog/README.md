# models.dev catalog snapshot

Offline copy of [models.dev](https://models.dev) (`https://models.dev/api.json`) for multi-provider discovery.

| File | Role |
|------|------|
| `models_dev.json.gz` | Shipped snapshot embedded in `xai-grok-models` and installed to `share/grok-build/` |
| `models_dev_fixture.json` | Small subset for unit tests only |

**Update rule (runtime):** fetch → parse → validate → only then replace `~/.grok/cache/models_dev.json.gz`. Never overwrite the cache with invalid JSON.

**Refresh (dev):**

```sh
curl -fsSL -A 'grok-build/catalog' -o /tmp/models_dev.json https://models.dev/api.json
# sanity-check JSON, then:
gzip -9 -c /tmp/models_dev.json > crates/codegen/xai-grok-models/catalog/models_dev.json.gz
```

Do not commit a broken gzip; `cargo test -p xai-grok-models` must pass.
