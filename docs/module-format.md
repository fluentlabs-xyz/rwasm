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

## Constructor/custom-section note

Constructor parameter conventions are handled in `src/types/constructor_params.rs`.
Treat constructor payload shape as ABI-level contract if consumed by external systems.
