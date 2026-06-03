# AGENTS.md

## Repository

This is `fluentlabs-xyz/rwasm`, the rwasm deterministic reduced WebAssembly format and runtime stack.

Normal work should be based on the latest `origin/devel` unless a manager or issue explicitly names another base branch. Before creating or updating a branch, fetch the remote base and make sure your branch is current.

## Working Rules

- Protect local work. Run `git status --short` before editing and again before reporting.
- Do not revert, reset, delete, or overwrite unrelated local changes. If unrelated changes block the task, stop and ask.
- Keep changes scoped to the issue. Avoid drive-by refactors and unrelated formatting churn.
- Use Conventional Commits for commit messages and PR titles, for example `docs: add rwasm agent instructions`.
- After opening or updating a PR, check and report the CI/check status.

## Setup

Expected local setup:

- Rust stable toolchain
- Rust nightly toolchain `nightly-2025-09-20`
- `wasm32-unknown-unknown` target installed for both stable and `nightly-2025-09-20`
- `clang`, `libclang-dev`, and `pkg-config`
- Initialized git submodules

Useful setup commands:

```bash
rustup target add wasm32-unknown-unknown
rustup +nightly-2025-09-20 target add wasm32-unknown-unknown
git submodule update --init --recursive
```

## Canonical Commands

Use the repository Makefile commands as the main local workflow:

```bash
make build
make clippy
make test
```

For small documentation-only changes, `git diff --check` is a reasonable local verification step, but still report that the full build/test commands were not run if you skip them.

## Repository Layout

- `src/` contains the compiler, module model, opcode types, VM, strategy layer, and Wasmtime integration.
- `e2e/` contains end-to-end harnesses and testsuite integration.
- `snippets/` contains snippet fixtures and tests that use the pinned nightly toolchain.
- `examples/` contains sample modules and programs.
- `benches/` contains Criterion benchmarks.
- `.github/workflows/` contains CI, clippy, coverage, benchmark, and publish workflows.
