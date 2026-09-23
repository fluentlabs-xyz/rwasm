---
name: bump-version
description: Bump the rwasm crate version and prepare a release (Cargo.toml, Cargo.lock, CHANGELOG.md, README proposal table, tag), following the rwasm versioning rule - major stays 0, minor only for instruction-set changes, patch for compatible fixes. Use when asked to bump, release, tag or publish rwasm.
---

# /bump-version

Prepare an rwasm release: pick the version by the rule below, bump it, write the changelog entry,
open the PR, and tag the merge commit so `publish.yml` publishes to crates.io.

## The versioning rule

The version is `0.MINOR.PATCH`. It describes **rwasm bytecode compatibility**, not the Rust API.

- **Major stays `0`.** It is not bumped to `1` for now, whatever changes.
- **Minor** goes up only when the **instruction set changes**: a new opcode, a removed or
  re-numbered opcode, a changed opcode encoding or immediate, a change to the module wire format,
  or a proposal enabled in `CompilationConfig::wasm_features` that lowers to new instructions.
  After a minor bump a module compiled by the new compiler may not decode or run on an older VM.
  Examples: 0.5.0 (`I32And64`/`I32Or64`/`I32Xor64`, `I32Sub64`, `I64Const32S/U`), 0.7.0
  (`I64Add128`, `I64Sub128`, `I64MulWideS`, `I64MulWideU`).
- **Patch** goes up for everything else: bug fixes in execution, compilation or validation that
  do not break compatibility of existing rwasm bytecode, performance work, fuel corrections,
  audit follow-ups, dependency bumps, docs. A patch may change the bytes the compiler emits for
  the same wasm (a different lowering, a new prologue) as long as every opcode already exists;
  say so in the changelog, because module hashes and `CompilationConfig::codegen_identity` move.
- A Rust API change on its own does not drive the version. Record it under **Changed** in the
  changelog and name what downstreams (fluentbase, the wasmtime fork) must adapt.

How to tell which one you have: `git diff vPREV..origin/devel -- src/types/opcode.rs docs/opcodes.md docs/module-format.md src/compiler/config.rs`. Any new `Opcode` variant, any changed code number, or a new `WasmFeatures` flag in the enabled union means **minor**; otherwise **patch**.

## Procedure

1. Start from the latest `devel` on a branch named `chore/bump-version-X.Y.Z`:
   ```bash
   git fetch origin devel && git switch -c chore/bump-version-X.Y.Z origin/devel
   ```
2. Collect what changed since the last tag and decide the version by the rule:
   ```bash
   PREV=$(git describe --tags --abbrev=0 origin/devel)
   git log --format='%s' $PREV..origin/devel
   git diff --stat $PREV..origin/devel -- src/types/opcode.rs docs/opcodes.md docs/module-format.md src/compiler/config.rs
   ```
3. Set `version = "X.Y.Z"` in `Cargo.toml` (the only place the version lives; `e2e/`, `fuzz/`
   and `snippets/` use path dependencies) and refresh the lockfile, which the publish job needs
   consistent because it runs `cargo publish --locked`:
   ```bash
   cargo check
   ```
4. Add the `## [X.Y.Z] - YYYY-MM-DD` entry at the top of `CHANGELOG.md`: one bullet per
   substantive PR under **Added**, **Changed**, **Fixed**, **Performance**, **Security** or
   **Docs**, with the PR number. Lead the entry with the compatibility consequence: which
   opcodes are new (minor), whether emitted bytes change for the same wasm (identity), and what
   downstream code must adapt. Skip dependabot noise unless it changes behaviour.
5. If a proposal's status changed, update the table in `README.md` (`## WebAssembly proposal
   support`); if opcodes were added, make sure `docs/opcodes.md` already lists them (that belongs
   to the feature PR, not the bump).
6. Commit, signed, as `chore: bump version to vX.Y.Z`; keep the changelog and README changes in
   their own `docs:` commits. Open a PR to `devel` with the same title, wait for CI (`make test`
   and the release-profile suite run there), and merge.
7. Tag the merge commit and push the tag. `publish.yml` runs on `v*` tags: it checks that the tag
   matches `Cargo.toml`, runs `make test`, then `cargo publish --dry-run` and `cargo publish
   --locked`:
   ```bash
   git fetch origin devel && git tag -s vX.Y.Z origin/devel -m "vX.Y.Z" && git push origin vX.Y.Z
   gh run list --workflow publish.yml --limit 1
   ```
   `workflow_dispatch` with `dry_run` rehearses the job without publishing.
8. Tell downstreams: fluentbase pins `rwasm = { version = "X.Y.Z" }` (root, `contracts/`,
   `e2e/evm/`, `examples/`) and re-runs its admission replay and `fluent re-execute` when
   validation or fuel changed; the wasmtime fork's `crates/cranelift/src/rwasm_fuel.rs` must match
   `rwasm-fuel-policy` if the fuel schedule moved.

## Do not

- Bump the version inside a feature PR; the bump is its own PR on top of `devel`.
- Use a `-dev` suffix; the dev-tag era ended with 0.4.0.
- Bump minor for a Rust API break or a lowering change that keeps the opcode set.
- Tag before the bump PR is merged, or tag a commit whose `Cargo.toml` disagrees with the tag:
  the publish job refuses it.
