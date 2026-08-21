# rWasm Security Audit — 2026-05-01

- **Date:** 2026-05-01 (external report); fixes landed 2026-05 → 2026-06
- **Repository:** `fluentlabs-xyz/rwasm` (primary); `fluentlabs-xyz/fluentbase`
  (mirrored `F-014`/`F-021` fixes)
- **Audited commit:** n/a (external PDF report `fluent-rwasm-audit-2026-05-01-en`)
- **Linear:** `FLU-32`, `FLU-33` (individual findings); `FLU-847` (umbrella fix
  ticket, `SECURITY_AUDIT.md` bundle)
- **Fix PRs:** rwasm #153; targeted Fluentbase fixes for `F-014` / `F-021`
- **Focus:** External audit of the rWasm compiler, ISA/opcode layer, VM/executor,
  and Wasmtime integration; two findings mirrored into the Fluentbase host surface.

## Scope

The external audit seeded the `F-###` finding IDs that later re-audits tracked to
closure (see the 2026-06-18 rWasm re-audit for the authoritative `F-###` closure
table). This report captures the individually-tracked fixes and the umbrella PR.

## Result

Severity counts were not tabulated per-ID in the external PDF; the tracked,
fixed items are listed below. The full `F-###` disposition (8 fixed, several
partial/open) is recorded in the 2026-06-18 rWasm re-audit.

## Findings

### High

#### F-021 — Built-in syscall handlers: wrap-around panic + unbounded allocation

- **Severity:** High · **Status:** Fixed · **Linear:** `FLU-32` · **Fix:** (Fluentbase)
- **Where:** syscall input parsing / allocation paths — `crates/revm/src/syscall.rs`,
  `crates/runtime/src/syscall_handler/**`, and the rWasm host integration.
- **Impact:** Attacker-controlled syscall sizes/offsets were used in arithmetic and
  buffer allocation without bounds checks → integer wrap-around panics and unbounded
  memory growth (OOM) from guest/syscall input.
- **Remediation:** Checked arithmetic + explicit maximum-size validation before
  allocation; oversized input maps to a deterministic VM/runtime error. Regression
  tests cover overflow/wrap-around and excessive-allocation inputs.

### Low / correctness

#### F-014 — `Opcode::code()` casts `&Opcode` to `*const u16`

- **Severity:** Low / correctness · **Status:** Fixed · **Linear:** `FLU-33` · **Fix:** (Fluentbase)
- **Where:** `Opcode::code()` in the opcode/types module (`#[repr(u16)]`).
- **Impact:** Unnecessary `unsafe` reading the discriminant through a raw pointer
  cast (author's own `TODO` flagged uncertainty); brittle if the enum layout changes.
- **Remediation:** Replaced with an explicit safe `match` preserving every
  discriminant; added tests locking the numeric values; removed the `TODO`. Zero
  `unsafe` remaining in `isa/` + `types/`.

### Compiler audit bundle

#### SECURITY_AUDIT.md findings — compiler/runtime panics, truncations, overflows

- **Severity:** mixed · **Status:** Fixed (bundle) · **Linear:** `FLU-847` · **Fix:** rwasm #153
- **Where:** rWasm compiler/runtime (`SECURITY_AUDIT.md` bundle).
- **Impact:** Compiler-side panics, truncations, and overflow classes reachable from
  untrusted Wasm.
- **Remediation:** Packaged into a single PR (rwasm #153), one commit per fix. The
  exact per-finding status (F1, F8, F9, F10, F12, F16, F19, F22, F26 fixed;
  F2/F3/F13 partial; F4/F5/F6/F7/F11/F14/F15 open) is the closure table in the
  2026-06-18 rWasm re-audit.

## Notes

Internal crate-intake audits from the same window — `FLU-30` (`crates/sdk-derive`)
and `FLU-31` (`crates/build`) — produced no tracked Medium+ findings and are not
reproduced as standalone reports.

## Re-review checklist for future audits

- Re-confirm every syscall handler validates attacker-controlled size/offset with
  **checked** arithmetic before allocating (F-021 class).
- Grep `isa/` and `types/` for reintroduced `unsafe` pointer casts on opcode/enum
  discriminants (F-014 class).
- Cross-check the `F-###` closure table in the 2026-06-18 re-audit before assuming a
  legacy finding is fixed.
