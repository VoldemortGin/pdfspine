# ADR 0005: Strict, source-bound paint profile

- Status: Accepted
- Date: 2026-09-19
- Applies to: page text/display-list interpretation and the additive pdfspine paint-audit API

## Context

Downstream evidence qualification must know when pdfspine has observed every
paint and graphics-state path that can affect a protected region. A native SVG
or replay stream alone cannot establish that fact: the content interpreter is
tolerant, unknown operators are skipped, malformed inline images are recovered,
and unresolved resources currently degrade to missing output. Generic xref
string access is also unsuitable because it loses typed reference, cycle and
scope information.

The page tree already resolves inherited `/Resources`, but the page text and
display-list entry points currently read only a leaf page's direct resource
dictionary. Forms may then replace resources or inherit their caller's resource
scope. A trustworthy audit must use the same effective page resources and Form
fallback rules as the actual interpreter.

## Decision

Add `Page.get_paint_profile()` as a pdfspine extension. It returns frozen PyO3
value objects whose collections are tuples and whose structured payloads are
read-only. The profile is produced by the Rust content parser; Python does not
tokenize PDF syntax.

The versioned profile contains:

- a typed resource graph for the effective page scope and actually reachable
  Form scopes, including direct, inherited and parent-fallback origins;
- operator observations with scope, ordinal, mnemonic and a supported,
  unsupported or malformed disposition;
- bounded ExtGState facts needed for paint accounting, including line width,
  cap, join, miter, alpha, blend mode and soft mask state, plus direct content
  observations for line dash, cap, join and miter operators;
- inline-image observations without image bodies; and
- stable diagnostics for lexer recovery, unsupported painting/state,
  malformed inline images, unresolved or wrongly typed resources, Form decode
  failures, cycles and depth limits.

`complete` means that every paint and graphics-state path reached by that page
was either represented by the audited interpreter or captured by an explicit,
versioned bounded observation. Direct `J`, `j` and `M` observations are the
latter: they let a downstream proof bound every legal stroke style, but do not
claim that replay or native SVG applied those values. The profile is false when
another relevant path is merely tokenized but not represented. In particular,
a parsed inline image is not sufficient if a downstream native or replay path
drops it. Unknown blend modes, soft masks, transparency groups, isolation and
knockout remain explicit unsupported diagnostics until their semantics are
implemented. This slice does not implement general transparency compositing or
full native-SVG stroke fidelity.

One bounded page-container case is accounted without claiming general
compositing: a page-level `/Group` whose actual typed values are exactly
`/Type /Group`, `/S /Transparency`, `/CS /DeviceRGB`, with `/I` and `/K` absent
or false and no unknown keys. Form groups, other colour spaces, true isolation
or knockout, unknown fields and malformed values remain unsupported. Executed
ExtGState alpha values must be exactly zero or one; values such as `0.999` are
not rounded into the opaque case. Text rendering modes 4–7 remain unsupported
because text clipping is not applied by the current audited renderer.

Declared resources are retained in the graph. An unused declaration alone does
not make the profile incomplete; an unresolved or unsupported resource selected
by `gs`, `Do`, `sh`, pattern colour or another executed operator does.

The Rust `Page` entry points materialize the existing guarded effective-resource
result for text, inventory, display-list, replay and paint profiling. The raw
`Page.dict()` result remains the unmodified leaf dictionary.

## Fail-closed limits

The initial profile is deliberately conservative. Unsupported content
operators, malformed content, unknown selected graphics state, nontrivial
`/Group`, unresolved references, recursive Form cycles and depth exhaustion
make the profile incomplete. Consumers may apply a separately reviewed bounded
geometry rule to recorded unknown cap/join state, but must not relabel the
profile as full native-SVG fidelity.

## Validation

Tests use authored PDFs and cover inherited page resources; Forms with their
own resources and parent fallback; direct and indirect resources; broken,
cyclic, wrong-type and depth-limited references; selected ExtGState including
unknown keys, blend and soft-mask state; `J`, `j`, `M`, `w`, dash and alpha;
unsupported operators; valid and malformed inline images; transparency groups;
and immutable Python results. Existing direct-resource rendering and replay
remain unchanged. The AIA p18 and p20 native-SVG digests are checked downstream
against the released v0.10.0 baseline before a new pdfspine release is used.

## Release boundary

This is an additive pdfspine extension requiring a new package release before a
downstream project may pin it. Repository tests and a release build are not a
PyPI publication. Tagging and publishing remain a separate maintainer action.
