# Frozen behavior baselines

`python-retry-ast.json` and `python-round4-pdf.json` preserve characterization data from the original Python application. They are provenance and comparison inputs, not current test reports, and must not be regenerated merely to make a new implementation pass. Original source and characterization tests are recoverable from Git tag `codex/baseline-final-integration-20261031`.

Current Rust retry/provider contracts consume `crates/core/tests/fixtures/legacy_retry_contract_v1.json`; compression fixtures remain beside that file. The historical Tencent xfail does not describe the current Rust implementation.

Automated PDF tests create synthetic documents in temporary directories. Manual regressions accept explicitly supplied PDFs. See [BUILD](../../docs/BUILD.md) for commands and [LIMITATIONS](../../docs/LIMITATIONS.md) for validation scope.
