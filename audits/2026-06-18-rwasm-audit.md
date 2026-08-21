# rWasm Security Audit — 2026-06-18

- **Date:** 2026-06-18
- **Repository:** `fluentlabs-xyz/rwasm`
- **Audited commit:** not recorded in the ticket (branch `devel`)
- **Linear:** `FLU-858`
- **Fix PRs:** #154
- **Focus:** Verify the fix-status of the prior external `F-###` findings (see the
  2026-05-01 rWasm audit) and add a fresh review pass. This is the authoritative
  closure record for the legacy `F-###` findings.

## Scope

Two jobs: re-verify the `F-###` findings from the 2026-05-01 external report, and a
new pass over the compiler, ISA, VM, and Wasmtime integration.

## Result

- Prior findings: **8 fully fixed**, 2 partial, several open (table below).
- New findings this pass: 1 Medium (verified) + 1 tooling-Critical (verified) + 5
  Low.

## Findings

### Prior `F-###` findings — closure table

| ID | Finding | Status |
| --- | --- | --- |
| F1 | `memory.init` `.expect()` compile panic | ✅ Fixed (`translator.rs` → `MemoryOutOfBounds`) |
| F8 | Host panic on missing memory/entrypoint | ✅ Fixed (`wasmtime/instance.rs` → `TrapCode`) |
| F9 | `program_counter` ptr→i32 truncation | ✅ Fixed (full-width `usize`) |
| F10 | `to_relative_address` truncation/underflow | ✅ Fixed (`checked_sub`/`checked_mul`) |
| F12 | `memory_size` u32 overflow | ✅ Fixed (`checked_mul` on usize) |
| F16 | Component-model `panic!` | ✅ Fixed (`Err(NotSupportedExtension)`) |
| F19 | `map_anyhow_error` `unreachable!` | ✅ Fixed (`_ => IllegalOpcode`) |
| F22 | `Opcode::code()` unsafe pointer cast | ✅ Fixed (safe `match`; zero `unsafe` in `isa/`+`types/`) |
| F26 | Nondeterministic `parse_function_exports` | ✅ Fixed (sorted) |
| F2/F13 | Bulk-op + builtin fuel metering | ⚠️ Partial (default flags off → undercharge) |
| F3 | rwasm-VM vs wasmtime fuel divergence | ⚠️ Partial (bulk-op divergence "fixed" by disabling metering on both) |
| F4 | NaN canonicalization | ❌ Open (only live under non-default `fpu`) |
| F5/F15 | CI hardening (self-hosted fork PRs, mutable tags) | ❌ Open |
| F6 | `deserialize_wasmtime_module` safe-looking unsafe API | ❌ Open |
| F7 | Infinite-loop DoS when `fuel_limit==None` | ❌ Open |
| F11 | wasmtime cache key omits config hash | ❌ Open (by design) |
| F14 | Fuzz: no raw-bytes/malformed target | ❌ Open |
| DEF1–6 | VM unsafe release-mode bounds net | ❌ Open (Low under trust model) |

### Medium

#### NEW-1 — Cross-engine memory-cap divergence → consensus split

- **Severity:** Medium (verified) · **Status:** Open (tracked) · **Linear:** `FLU-858` · **Fix:** —
- **Where:** `wasmtime/instance.rs` clamps `max_allowed_memory_pages` to
  `N_MAX_ALLOWED_MEMORY_PAGES` (32768), but `vm/store.rs` passes it straight to
  `Pages::new_unchecked` with **no clamp**.
- **Impact:** If a caller sets pages > 32768, `memory.grow` into that band succeeds
  under the rwasm VM but traps under wasmtime → divergent results between strategies
  (consensus split).
- **Remediation:** Apply the same `.min(N_MAX_ALLOWED_MEMORY_PAGES)` in
  `RwasmStore::new`.

### Critical (tooling)

#### NEW-2 — Fuzzer wasmtime version skew (regression)

- **Severity:** Critical for tool correctness (verified) · **Status:** Open (tracked) · **Linear:** `FLU-858` · **Fix:** —
- **Where:** root uses `wasmtime-rwasm 45.0.0-rwasm.1` but `fuzz/Cargo.toml` still
  pins `41.0.2-rwasm.3`; `fuzz/Cargo.lock` resolves both.
- **Impact:** The differential fuzzer's oracle is wasmtime **41** while rwasm links
  **45** → divergences may be 41-vs-45 drift and real rwasm/45 bugs are masked.
- **Remediation:** Bump fuzz to `45.0.0-rwasm.1`, regenerate lock, add a CI equality
  check.

### Low

#### NEW-3 — Const-expression recursion can overflow the native stack at compile time

- **Severity:** Low (low confidence) · **Status:** Open (tracked) · **Linear:** `FLU-858` · **Fix:** —
- **Where:** `compiler/compiled_expr.rs` builds nested boxed closures.
- **Impact:** A const expr with ~100k chained ops recurses deep → compiler
  stack-overflow abort (DoS).
- **Remediation:** Cap operator count or evaluate iteratively.

#### NEW-4 — Tracer `record_mr` loop is dead/inverted (`for idx in length..0`)

- **Severity:** Low (latent proof-soundness) · **Status:** Open (tracked) · **Linear:** `FLU-858` · **Fix:** —
- **Where:** `vm/tracer/mod.rs`.
- **Impact:** Empty range for non-negative length → stack-read records never emitted
  (dead code today; soundness gap if wired into proof generation). Also ungated
  `println!` spam in the same file.
- **Remediation:** Fix the loop bounds; gate the debug print.

#### NEW-5 — `table.init` `.expect()` still present

- **Severity:** Low (not currently reachable) · **Status:** Open (tracked) · **Linear:** `FLU-858` · **Fix:** —
- **Where:** `compiler/translator.rs`.
- **Impact:** Inconsistent with the F1 fix.
- **Remediation:** Convert to `.ok_or(TableOutOfBounds)?`.

#### NEW-6 — Dead `as_u16`/`From<u64>` silent-truncation footguns

- **Severity:** Low · **Status:** Open (tracked) · **Linear:** `FLU-858` · **Fix:** —
- **Where:** `types/untyped_value.rs`.
- **Impact:** No `debug_assert` on the high bits.
- **Remediation:** Add high-bit assertions or remove the dead conversions.

#### NEW-7 — `dependabot.yml` covers only `github-actions`, not `cargo`

- **Severity:** Low · **Status:** Open (tracked) · **Linear:** `FLU-858` · **Fix:** —
- **Where:** `.github/dependabot.yml`.
- **Impact:** Rust deps get no automated security bumps (compounds the
  gitignored-lockfile gap).
- **Remediation:** Add a `cargo` ecosystem entry.

## Notes

**8 findings fully fixed** (the untrusted-WASM-reachable compiler/runtime panics and
the truncation/overflow class), closing the highest-priority functional bug (F1).
Open items in priority order (as filed): (1) NEW-1 + F3/F2/F13 consensus/gas risks;
(2) F7 infinite-loop DoS; (3) F5/F15 CI trust model + NEW-2 fuzzer skew; (4) F4 NaN
(only if `fpu` is enabled); (5) F6, F11, F14, DEF1–6, and the NEW low items. Several
of these were carried forward and driven to closure in the 2026-08-07 rWasm audit
(bulk-op fuel divergence → `FLU-1101`/`FLU-1104`, cache config identity →
`FLU-1103`/`FLU-1107`, panicking public APIs → `FLU-1108`).

## Re-review checklist for future audits

- Confirm `RwasmStore::new` clamps memory pages identically to the wasmtime path
  (NEW-1).
- Confirm the fuzzer and the library link the **same** wasmtime-rwasm version (NEW-2).
- Confirm bulk-op fuel metering is on by default and identical across both execution
  strategies (F2/F3/F13).
- Re-check `deserialize_wasmtime_module`'s trust contract (F6).
