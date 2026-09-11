# Published and local validation baselines

These are separate checkpoints; a local development gate is not the gate for a
published release, and earlier full-suite counts do not describe every later HEAD.

| Scope | Source | Implemented / baseline | Deferred |
|---|---|---:|---:|
| Published v0.8.0 | `f1f6ab4208876b0ba867edd76cc4e5da7ad8add2` | 694 / 769 (90.2%) | 9 |
| Current local source catalog | [COMPAT.toml](../COMPAT.toml) | 703 / 769 (91.4%) | 0 |

Both retain 66 out-of-scope symbols. The nine later implementations are
unreleased; some, such as replay, use explicitly documented pdfspine contracts
rather than native-handle compatibility. See [PARITY](../PARITY.md) and the
[release history](../CHANGELOG.md#080--2026-09-10).

## Most recent recorded full gate

The **2026-09-11 local signed-spacing gate** passed **2,027 Rust tests and 1,518
Python tests**, with **68 existing Python skips**. Rust, extension, Python,
drift and wheel/sdist installation checks passed. This is a named unreleased
checkpoint, not a claim that the current HEAD has run the complete suite.

- Tested source: `0c320dcb0fe7a8eb714b2469116f9544eda1190d`.
- Local integration: `e67545f13f713d4e1a6f92a21bdeb9a7ce07c90e`.
- Extension input fingerprint:
  `7a92be9fe832d8b2e720b26ad5fb6222490bdaecd62f6419ffa644e031c160d3`.
- Actual native extension SHA256:
  `fe8ba4d7c88df62c88e4defbd2b73d33e7748369e3156351010d91a2b5eaf5d6`.
- Archived log: `typeset-negative-tracking-readonly/signed-main-final-gate.log`,
  SHA256 `9ce663f3557da14a70f78df4c6c1b5765e7b4edb75dd5764b620c24b546fe68f`.
  Its companion `signed-final-integration.json` records phase results and counts.
  These external evidence paths are relative to the retained experiment archive
  (`/Volumes/ExternalSSD/tmp` on the collection machine), not installed-package
  requirements.

The subsequent evaluator-only TableFormer integration
`35e5f53568d9e174b00df8fdbc8d47e24ebd986e` reused that native binary and passed
**132 related tests with 5 existing model skips**. These are targeted checks,
not another complete suite or numbers to add to the full-gate totals. Later
pure documentation changes likewise do not imply a new full gate. Model
readiness and unreviewed table diagnostics are separate evidence in
[TABLE-TSR](../conformance/gt/TABLE-TSR.md) and the
[diagnostic report](../conformance/gt/TABLE-DIAGNOSTICS-2026-09-11.md).

Historical release-gate numbers remain in their dated records. No replacement
v0.8.0 release-gate count is inferred from these later local runs.
