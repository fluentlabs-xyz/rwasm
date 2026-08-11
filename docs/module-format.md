# Module Format

Canonical implementation: `src/module/mod.rs`.

`RwasmModule` is a wrapper around `RwasmModuleInner` and is encoded with bincode (`legacy` config).

## Binary header

Each encoded module starts with:

1. Magic byte 0: `0xEF`
2. Magic byte 1: `0x52` (`'R'`)
3. Version: `0x01`

Decode fails if magic/version do not match.

## Encoded payload order

After header, fields are encoded in this exact order:

1. `code_section: InstructionSet`
2. `data_section: Vec<u8>`
3. `elem_section: Vec<u32>`
4. `hint_section: Vec<u8>`
5. `source_pc: u32` (optional for legacy blobs; defaults to `0` if missing)

## Section meaning

- **code_section**: compiled opcode stream (entrypoint + called functions)
- **data_section**: read-only linear memory initialization bytes
- **elem_section**: table element initializer values (function references)
- **hint_section**: original source-hint payload (e.g., original wasm bytes)
- **source_pc**: source entry offset hint in compiled stream

## Compatibility notes

- Field order and opcode layout are part of wire compatibility.
- Feature combinations (`fpu`, etc.) alter executable surface and should be pinned.
- Legacy support currently handles missing `source_pc` by defaulting to `0`.

## Codegen determinism

The header version (`0x01`) identifies the *wire format*, not the compiler configuration. The same
wasm input does not compile to the same bytes under every build, and the encoded module records
nothing about the build that produced it.

Inputs that change emitted bytecode:

- **`CompilationConfig` flags** — `code_snippets` decides which functions are emitted and therefore
  shifts every downstream instruction offset; `consume_fuel`, `consume_fuel_for_bulk_ops`,
  `consume_fuel_for_params_and_locals`, and `builtins_consume_fuel` decide where fuel charges are
  injected; `max_allowed_memory_pages` and `default_imported_global_value` are baked into the
  emitted code; `state_router`, `entrypoint_name`, and `import_linker` shape the entrypoint and the
  syscall mapping. The `allow_*` flags relax validation and decide which inputs compile at all.
- **The `fpu` cargo feature** — off (the default), every float instruction is compiled to
  `Trap(IllegalOpcode)`; on, real float opcodes are emitted. Floating point is not officially
  supported: `fpu` exists for the e2e suite and the fuzzer only and must not be enabled in a
  production build.

Features that only change the host-side surface (`std`, `serde`, `wasmtime`, `debug-print`, …) do
not affect the emitted bytes.

`CompilationConfig::codegen_identity()` hashes all of the above — the codegen-relevant config fields
plus the compile-time feature set of the compiling binary — into a 32-byte fingerprint. Compilers
agreeing on this value produce identical bytecode for a given wasm input. The fingerprint is **not**
part of the wire format, so a host that addresses modules by hash must carry the identity alongside
the bytecode and reject a module whose producer identity does not match its own. Embedding the
identity in the header would require a `RWASM_VERSION_V2` bump and would change the bytes (and
hashes) of every module.

## Constructor/custom-section note

Constructor parameter conventions are handled in `src/types/constructor_params.rs`.
Treat constructor payload shape as ABI-level contract if consumed by external systems.
