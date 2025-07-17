# rWasm (Reduced WebAssembly)

[![codecov](https://codecov.io/gh/fluentlabs-xyz/rwasm/graph/badge.svg?token=9T2PLQQW4L)](https://codecov.io/gh/fluentlabs-xyz/rwasm)

**rWasm** is a ZK-friendly binary intermediate representation (IR) of WebAssembly (Wasm), designed for fast execution
and efficient zero-knowledge proof generation.
It preserves full semantic compatibility with Wasm while removing
non-deterministic and hard-to-prove elements.

## Key Highlights

* **ZK-Focused**: Flattened structure and simplified control flow optimized for proving.
* **Full Wasm Compatibility**: Every Wasm feature is either preserved or safely substituted.
* **rWasm Runtime**: Designed to be used in zkVMs and optimized interpreters.
* **EIP-3540 Compatible**: Follows the modular structure introduced by Ethereum’s Wasm compatibility standards.

---

## Motivation

WebAssembly is an attractive binary format thanks to its structured control flow, clear memory model, and rich ecosystem
support.
However, its current binary design (sectioned modules, type indices, relative branches) introduces complexity
for ZK proving.
Features like:

* Relative jump targets
* Indirect function calls
* Type-table indirection
* Imports/exports with dynamic semantics

...make Wasm difficult to validate and trace deterministically in zero-knowledge systems.

### rWasm addresses these challenges by:

* Flattening control flow.
* Embedding all necessary metadata inlined with bytecode.
* Eliminating the need for post-decode validation.

---

## Core Design Principles

### Deterministic Layout

* Functions are inlined into a flat bytecode section.
* All branch targets are PC-relative.
* Control structures (`block`, `loop`, `if`) are desugared into explicit `br` sequences.

### No Type Mapping

* Function types are validated at rWasm compile-time and inlined—no external type section is needed.
* The module is immediately executable without prior type resolution.

### No Dynamic Imports

* rWasm is self-contained: no `import` or `export` sections are required for execution.
* All external dependencies must be pre-resolved.

---

## Binary Structure

| Section        | Purpose                                 |
|----------------|-----------------------------------------|
| Bytecode       | Flat instruction stream                 |
| Function Index | Length table used for function recovery |
| Memory         | Merged memory and data segments         |
| Element        | Optional: Table segment placeholder     |

Future versions aim to remove the Function and Element sections entirely.

---

## Control Flow Rewriting

* All structured blocks are rewritten into `br`, `br_if`, and `br_table`.
* Break targets use relative PC offsets instead of relative depth.

Example:

```wasm
(block $label
  ...code...
  br $label
)
```

Becomes:

```wasm
...code...
br @relative_pc_offset
```

---

## Entrypoint Model

A special function (`__entrypoint`) is injected at the end of the bytecode. It:

* Initializes memory, tables, and globals.
* Prepares runtime memory (e.g., for passive segments).
* Starts execution from a user-defined main function.

We are currently working on making the entrypoint the first function (offset 0) to reduce proving overhead.

---

## Memory Handling

### Flattened Memory Section

* Merges memory declaration and data segments.
* Only one contiguous memory segment is allowed (no memory imports).
* Upper bounds are enforced via injected `memory.size` checks.

### Example:

```wat
(memory 1 2) ;; min: 1 page, max: 2 pages
```

Generates:

```wat
(func $__entrypoint
  ;; grow memory
  i32.const 1
  memory.grow
  drop
  ;; ...
)
```

P.S.: The provided snippets are for conceptual illustration only. Actual implementations may differ—refer to the
codebase for details.

### Data Segment Handling

* **Active segments**: Initialized in the entrypoint.
* **Passive segments**: Offset-mapped at runtime with injected guards.
* Data segment drops are explicitly emitted after init.

---

## Future Plans

* **Register-based IR**: Replace the stack machine with a linear register model for better AOT support and ZK
  friendliness.
* **Bytecode Streaming**: Enable lazy evaluation or streaming bytecode execution for large programs.
* **Syscall Injection**: Full syscall layer for host environment interaction (e.g., EVM, SVM integration).
* **Element and Function Section Elimination**: Reduce binary size and remove unnecessary bookkeeping.

---

## Runtime Integration

rWasm is validated and executed using a custom interpreter or proving backend (e.g., Wasmi, rwasm-vm). It maintains
compatibility with Wasmi’s IR but replaces the module loader and validator pipeline.

It is used in the [Fluent](https://github.com/fluentlabs-xyz) project to execute smart contracts in a unified runtime
that supports EVM, SVM, and Wasm logic in a ZK-friendly way.