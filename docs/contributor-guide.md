# Contributor Guide

## Prerequisites

- Rust toolchains:
  - stable
  - nightly `nightly-2025-09-20`
- wasm target:
  - `wasm32-unknown-unknown` for both toolchains
- build dependencies:
  - `clang`, `libclang-dev`, `pkg-config`
- git submodules initialized (`e2e/testsuite`)

## One-time setup

```bash
rustup target add wasm32-unknown-unknown
rustup +nightly-2025-09-20 target add wasm32-unknown-unknown
git submodule update --init --recursive
```

## Canonical local commands

```bash
make build
make clippy
make test
```

## What each command does

- `make build`
  - ensures wasm targets and submodules
  - checks/builds root + examples + snippets
- `make clippy`
  - runs clippy for root, e2e, snippets
- `make test`
  - runs root tests, e2e tests, snippet tests, and release nitro verifier test

## CI expectations

Default PR checks should pass:

- test workflow
- clippy workflow
- (optional) coverage/bench depending on trigger and permissions

## Change policy

When modifying any of these surfaces, update docs in same PR:

- opcode set / instruction semantics
- module encoding/layout
- fuel/trap behavior
- CI command shape / toolchain requirements

## PR checklist

- [ ] `make build` passes
- [ ] `make clippy` passes
- [ ] `make test` passes
- [ ] docs links valid in `docs/`
- [ ] release-impact note if format/behavior compatibility changed
