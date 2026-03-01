# Phase 1 — Delivery Audit (rwasm)

This document captures a concrete audit baseline before structural changes.

## Scope audited

- Repository layout and build graph
- Existing docs (`README.md`, `docs/*`)
- CI/CD workflows (`.github/workflows/*`)
- Local quality gates (`Makefile`, `cargo clippy`, `make test`)
- Runner compatibility assumptions

## Findings

## 1) Documentation quality and drift

- `README.md` mixes product messaging, benchmark claims, and implementation details without clear source-of-truth references.
- `docs/` has useful content, but structure is fragmented:
  - no single architecture flow document from "Wasm input -> compiler -> rWasm module -> VM/wasmtime execution"
  - instruction docs are split into many files with inconsistent naming style
  - opcode information is not centralized into one canonical machine-readable-like spec page
- No explicit "stability guarantees" section (format/versioning compatibility, feature gates, and known limitations).

### Impact

Harder onboarding and harder external trust review. Maintenance cost grows because readers cannot quickly answer "what is canonical?".

## 2) Clippy and lint gate configuration

- CI linting is not consistently enforced for normal PR runs in existing setup (lint job previously tied to workflow_dispatch only).
- Clippy invocation strategy is not codified as one canonical delivery gate (strictness, targets, features).

### Impact

Style and correctness regressions can bypass default PR pipeline.

## 3) CI/CD runner assumptions

- Workflows mix runner labels (`ubuntu-latest`, `ubuntu-amd64-8core`) and implicit host packages.
- Toolchain/system deps (notably clang/libclang) are required by downstream crates and tests.
- Fork PR permissions can break comment-posting steps (sticky comment action) even when benchmark command itself is fine.

### Impact

Red pipelines from environment mismatch rather than code regressions.

## 4) Test pipeline reproducibility

- `evm-e2e` suite depends on external Ethereum test corpus sync step.
- If sync is skipped, tests fail with filesystem errors despite valid code.

### Impact

Non-deterministic CI outcomes and noisy failures.

## 5) "Finished repo" gaps

- No explicit docs index for users/contributors/operators.
- No single opcode catalog page designed for protocol/tooling implementers.
- No clear phased definition for "done" state per repository surface (code/docs/CI).

---

## Target outcomes for next phases

## Phase 2 (Quality Gates)

- Enforce clippy in regular PR CI (strict, reproducible command).
- Align runner labels and package/toolchain requirements with actual infra.
- Keep failing steps actionable (no permission-noise on forks).

## Phase 3 (Docs rewrite)

- Rewrite `docs/` into a coherent set:
  - architecture
  - compilation pipeline
  - binary/module encoding
  - opcode specification (single canonical page)
  - execution model and fuel/traps
  - contributor/operator quickstart

## Phase 4 (Finish pass)

- Consistency cleanup (naming, broken references, stale statements).
- Final validation: docs links + clippy + tests + CI run shape.

---

## Definition of done for this delivery

- Clear docs navigation and canonical opcode spec present.
- Clippy gate configured and passing in CI default PR path.
- Runner-compatible workflows for current infra label strategy.
- Test command reproducible locally and in CI.
- PR leaves repository in "reviewable/ship-ready" state, not "works only on maintainer machine".
