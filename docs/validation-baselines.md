# Published and local validation baselines

These are separate checkpoints; a local development gate is not the gate for a
published release, and earlier full-suite counts do not describe every later HEAD.

| Scope | Source | Implemented / baseline | Deferred |
|---|---|---:|---:|
| Published v0.11.2 | `78a64d6e252ab739fcbad66c0d7f5328a080d667` | 703 / 769 (91.4%) | 0 |
| Current local source catalog | [COMPAT.toml](../COMPAT.toml) | 703 / 769 (91.4%) | 0 |

Both retain 66 out-of-scope symbols. The published v0.11.2 catalog now matches
the current local source; some implementations, such as replay, use explicitly
documented pdfspine contracts rather than native-handle compatibility. See
[PARITY](../PARITY.md) and the [release history](../CHANGELOG.md#0112--2026-09-24).

## Most recent recorded full gate

The **2026-09-11 local paragraph-connection gate** passed **2,046 Rust tests and
1,535 Python tests**, with **68 existing Python skips**. All five phases passed:
Rust fmt/clippy/tests/deny, extension, Python, drift, and installed wheel/sdist
smokes. The complete vendor inventory was included in the sdist checks. This
is an unreleased checkpoint for the additive checked-only Rust connection API,
not a new published-release validation.

- Tested source / implementation: `267961bb5ead0b83f9bdc414d69e0a464ba505fe`.
- Extension input fingerprint: `792abbb2b8eb8bfb31ee0916e31dcebedffc10755f888bd544f1a9ad2cb0a9de`.
- Actual native extension SHA256: `fe8ba4d7c88df62c88e4defbd2b73d33e7748369e3156351010d91a2b5eaf5d6`.
- Archived log: `typeset-between-readonly/main-final-gate.log`, SHA256
  `3b071ae30896be56e2faeb7f1b17a92100e8bc278a66936f8e135a2fa57a8da5`; `final-integration.json` records the local merge and phase results.
  These paths are relative to the experiment archive described below.

The extension was rebuilt and verified. Its SHA remains unchanged because
`pdf-typeset` is not a Python-extension dependency. Twelve focused Rust cases
and the crate's 229 tests cover the new behavior. The four old checked return
types still compile/run against the candidate. Eleven previous generated PDFs
and all twelve old reference buffers remain byte-identical; the full 13-page
reference check passes at explicit **100dpi**, minimum SSIM **0.9991**. The new
fixture reads back `A paragraph. B paragraph.`. The first reference invocation
incorrectly used 150dpi and mismatched old dimensions; only the new reference
and invocation were corrected to the existing 100dpi contract. No old reference
was regenerated. Subsequent checkpoint edits do not change product sources.

See [paragraph connections](typeset-paragraph-connections.md) for the same-page
38→39pt and changing-page 1→5-line red/green evidence, the limited LO comparison,
provider-error boundary, and unsupported raw OOXML / variable-width policies.

## Earlier paragraph-dash full gate

The **2026-09-11 local paragraph-dash gate** passed **2,034 Rust tests and 1,535
Python tests**, with **68 existing Python skips**. All five phases passed: Rust
fmt/clippy/tests/deny, extension, Python, drift and wheel/sdist installation.
The sdist vendor inventory and both installed-artifact smoke checks passed.
This includes the earlier TableFormer evaluator tests; it is an unreleased
source checkpoint, not a replacement release gate.

- Tested source: `472a47f24b41f07dcc1dc21375d95a949cc0ddc1` (implementation `0d5d584`).
- Extension input fingerprint: `4ed006d6cc62c1609691433330a0e71dcb1ed6f7fcdb71270328788c1a550805`.
- Actual native extension SHA256: `fe8ba4d7c88df62c88e4defbd2b73d33e7748369e3156351010d91a2b5eaf5d6`.
- Archived log: `typeset-dash-probe/main-final-gate.log`, SHA256
  `d43b04e81e0c38a8f95ae5fbbef4665950ea9a6676bb4d951cf5c3f8debe267c`; companion `final-integration.json` records the final
  local merge and phase results. Paths use the experiment archive described below.

The extension was rebuilt against its recorded inputs. Its SHA is unchanged
from the earlier checkpoint because `pdf-typeset` is not a dependency of the
Python extension; the new Rust typeset behavior is covered by workspace tests.
Final-binary readback passes 9/9 PDFs and render references 12/12 pages (minimum
SSIM 0.9991). Ten prior generated PDFs remain byte-identical. Validation-report
edits after this gate do not change Rust/Python product sources.

## Earlier signed-spacing full gate

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
