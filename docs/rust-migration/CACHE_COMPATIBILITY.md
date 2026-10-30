# Rust persistent cache contract

Phase 6 adds optional SQLite persistence to `pdf2zh-core`. This document describes
the implementation and its compatibility decisions. The parent integration run
passed all 167 Rust workspace tests, including all 27 cache tests, strict
`cargo clippy --workspace --all-targets -- -D warnings`, and formatting checks.
The Python worker boundary suite also passed all 26 tests. Evidence is recorded
in `.test-output/rust-migration/phase6-core-cli-tests.log`; this is offline
verification, not a real-provider API certification. No cache test reads a user's
database, configuration or credentials.

## API and ownership

`CacheStore::open(path)` opens an explicitly supplied path. The core never searches
the home directory or chooses the Python cache path automatically. Opening errors
are returned to the caller. `open_read_only(path)` supports inspection and reuse
without schema changes. The caller can attach a store using:

```rust,ignore
let store = Arc::new(CacheStore::open(path)?);
let legacy_present = store.legacy_present();
let service = TranslationService::new(config)?.with_cache(store);
```

`with_cache` accepts `Arc<dyn CacheBackend>` for injection. Without it, the service
does no cache I/O. `requires_cooperative_cleanup()` includes attached cache
backends as well as processes; `requires_reap()` and `owns_process()` retain their
process-only meanings. The bridge must not abort an operation owning cache cleanup.

## Observable translation sequence

The service counts the call, checks cancellation and validates the method, then:

1. Unless instance `options.ignore_cache` or request `ignore_cache` is true, read
   the cache. A hit returns its string, including an empty string.
2. On a miss, run provider preflight, acquire any configured concurrency permit,
   and obtain one QPS admission for the entire logical translation.
3. Execute the provider and all its SDK/application retries.
4. Store successful text unless caching is ignored. Provider errors, incomplete
   responses and provider cancellation do not reach this write.

Hits do not obtain an admission, make provider/preflight HTTP calls, enter provider
inflight accounting, or increment token usage. Their returned `Usage` is zero.
Calling `prepare()` explicitly remains a separate startup operation; the cache
cannot undo a caller's earlier explicit preflight.

There is no TTL, eviction or singleflight merge in this phase. Two simultaneous
misses can make two provider requests. Each successful write is atomic; the last
committed write determines the later cache value. A concurrent miss is never
reported as a cache hit.

## Errors are handled at two layers

The Python source, rather than the earlier migration matrix shorthand, defines
the actual behavior:

| Layer | Python behavior | Rust counterpart |
|---|---|---|
| `TranslationCache.get` | Database exceptions escape | `CacheStore::get` returns a safe typed error |
| `BaseTranslator.translate` / `llm_translate` read | Catches `Exception`, then translates | Service increments `cache_read_errors`, then translates |
| `TranslationCache.set` | Catches `Exception` internally | Store exposes the error for diagnostics; service increments `cache_write_errors` and returns the valid provider text |
| Cancellation | New control event, separate from provider exceptions | `CacheErrorKind::Cancelled` becomes provider `Cancelled` and is never swallowed |

The best-effort rule also applies to injected backends. This is an explicit Rust
backend contract: a custom backend's ordinary returned error does not fail a
translation. Error text contains neither SQL, path, source text nor credentials.
Opening a store is configuration work and does not silently fall back to a
different database.

SQLite calls run in blocking workers behind a cancellable per-store gate. Busy
and locked responses retry asynchronously for up to the Python one-second busy
budget. Cancelled workers are interrupted and awaited; their transaction is
committed or rolled back before the service exposes cancellation. A write checks
the cancellation token before committing. A commit that has already completed
cannot be retracted if cancellation races with its completion; there is no writer
left running after cancellation returns. Cache wait time is not provider retry
sleep or QPS admission time.

## Rust namespace and data at rest

The new table is separate from the Python table:

```sql
CREATE TABLE rust_translation_cache_v2 (
    namespace TEXT NOT NULL,
    original_text TEXT NOT NULL,
    translation TEXT NOT NULL,
    PRIMARY KEY (namespace, original_text)
) WITHOUT ROWID;
```

`namespace` is a SHA-256 digest of a versioned identity containing provider,
endpoint, model, input/output languages, method, caller role, prompt template,
output-affecting options and request parameters. Method aliases are normalized;
`translate` and `llm_translate` have different identities, as do main and term
roles. Output options include nested `extra_body`, JSON mode, model controls,
CLI command and postprocess configuration. The namespace contract must be bumped
when provider behavior changes in a way requiring invalidation.

The explicit API key and recognized credential fields are excluded. Operational
QPS, concurrency, ignore-cache, timeout and cache-path options do not partition
the output cache. Other configuration is represented only by the digest: raw
endpoint, prompt/options and header values are not stored. Original input text
and translated text are intentionally stored, as in the Python cache. Debug
implementations redact input and namespace parameters. SHA-256 is the namespace
collision boundary; canonical configuration is not stored as a secondary
plaintext collision check.

## Python database compatibility

The Python table is `_translationcache`, with `id`, `translate_engine`,
`translate_engine_params`, `original_text` and `translation`, and a unique
engine/params/text tuple using `ON CONFLICT REPLACE`. Rust neither rewrites nor
deletes this table. Reading an active WAL database uses SQLite itself, not a file
copy or a forced checkpoint.

`LegacyNamespace::new(engine, params)` reproduces recursively sorted Python
`json.dumps` formatting: ASCII escapes, UTF-16 surrogate pairs, comma/colon
spaces, array order, and float notation including the scientific-notation
boundaries. The engine length check is Python's fewer-than-20 Unicode characters.
`null` at the constructor boundary means the original default empty parameters.
Inputs are Rust `serde_json::Value`; arbitrary-size Python integers and unpaired
Unicode surrogates outside that representation are not implicitly coerced into
matching namespaces.

`read_legacy(namespace, text, cancel)` provides exact read access. Legacy rows
lack endpoint, request method, role and some later output options, so their mere
presence is insufficient to prove they belong to a current service. The default
service therefore does not reuse them. `legacy_present()` lets the caller report
this state instead of silently treating an old row as a verified match.

An explicit caller can bind verified provenance:

```rust,ignore
let namespace = LegacyNamespace::new("openai", &verified_python_parameters)?;
let binding = LegacyBinding::new(namespace, CacheMethod::Translate, "main");
let service = service.with_legacy_cache(binding);
```

This binding applies only to that configured service, method and role. On a Rust
miss, it reads the exact Python tuple and promotes the result to the complete Rust
namespace. The old row remains unchanged. `legacy_cache_hits` records this reuse.
The binding must not be inferred solely from the endpoint currently selected in a
new configuration; the old database cannot establish that provenance.

Python's two translation entry points originally share one cache object and
namespace. Splitting method and role in the Rust namespace is an intentional
compatibility change preventing cross-context reuse. Legacy migration requires
the explicit method/role binding for the same reason.

## Verification index

- `crates/core/tests/cache_storage.rs`: Python formatting, finite float
  boundaries, legacy table/active WAL, persistent overwrite/empty output, read
  only/corrupt/missing-schema errors, namespace separation and credential
  exclusion, concurrent connections, busy budget, gate/write cancellation.
- `crates/core/tests/cache_service.rs`: actual service ordering, hit usage/QPS
  behavior, ignore-cache reads and writes, injected and real SQLite failures,
  retries, provider cancellation, SQLite cleanup, concurrent same-key misses,
  explicit legacy promotion, and one real loopback HTTP request followed by a
  cache hit after reopening the database and service.
- Bridge cancellation must additionally be tested across the IPC boundary with
  cooperative cleanup; core tests alone do not prove the bridge avoids aborting
  the cache owner.
