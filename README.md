rWASM (reduced-WebAssembly)
===========================

rWASM (reduced WebAssembly) is an EIP-3540 compatible binary intermediary representation (IR) of WASM (WebAssembly).
It is designed
to simplify the execution process of WASM binaries while maintaining 100% compatibility with original WASM features.

## Key Features

- **ZK-Friendliness**: rWASM achieves Zero-Knowledge (ZK) friendliness by having a more flattened binary structure and a
  simplified instruction set.
- **Compatibility**: rWASM retains full compatibility with WASM, ensuring that all original WASM features are preserved.

## Important Notice

rWASM is a trusted execution runtime and should not be run without proper validation.
It is safe to translate WASM to rWAS M and execute, as rWASM injects all necessary validations into the entrypoint.

# Motivation

WebAssembly (WASM) is an interpreted language and binary format favored by many Web2 developers.
Our approach aims to seamlessly integrate these developers into the Web3 world,
despite the challenges this integration presents.
We prefer WASM over RISC-V or other binary formats due to its well-established and widely adopted standard,
which developers appreciate and support.
Moreover, WASM includes a self-described binary format (covering memory structure, type mapping, and more),
unlike RISC-V/AMD/Intel binary formats,
which require binary wrappers like EXE or ELF.

However, WASM is not without its drawbacks,
particularly its non-ZK friendly structures that complicate the proving process.
This is where rWASM (Reduced WebAssembly) comes into play.

## Introducing rWASM

rWASM is a specially modified binary intermediary representation (IR) of WASM execution.
It retains 99% compatibility with the original WASM bytecode and instruction set
but features a modified binary structure that avoids the pitfalls of non-ZK friendly elements,
without altering opcode behavior.

The main issue with WASM is its use of relative offsets for type mappings, function mappings, and block/loop statements,
which complicates the proving process.
rWASM addresses this by adopting a more flattened binary structure without relative offsets
and eliminating the need for a type mapping validator,
allowing for straightforward execution.

## Benefits of rWASM

The flattened structure of rWASM simplifies the process of proving the correctness of each opcode execution
and places several verification steps in the hands of the developer.
This modification makes rWASM a more efficient and ZK-friendly option
for integrating Web2 developers into the Web3 ecosystem.

# Technology

rWASM is built on WASMi's intermediate representation (IR),
originally developed by [Parity Tech](https://github.com/wasmi-labs/wasmi) and now under Robin Freyler's ownership.
We chose the WASMi virtual machine because its IR is fully consistent with the original WebAssembly (WASM),
ensuring compatibility and stability.
For rWASM, we adhere to the same principles,
making no changes to WASMi's IR and only modifying the binary representation to enhance zero-knowledge
(ZK) friendliness.

### Key Differences:

1. **Deterministic Function Order**: Functions are ordered based on their position in the codebase.
2. **Block/Loop Replacement**: Blocks and loops are replaced with Br-family instructions.
3. **Redesigned Break Instructions**: Break instructions now support program counter (PC) offsets instead of
   depth-level.
4. **Simplified Binary Verification**: Most sections are removed to streamline binary verification.
5. **Unified Memory Segment Section**: Implements all WASM memory standards in one place.
6. **Removed Global Variables Section**: Global variables section is eliminated.
7. **Eliminated Type Mapping**: Type mapping is no longer necessary as the code is fully validated.
8. **Special Entrypoint Function**: A unique entry point function encompasses all segments.

The new binary representation ensures a 100% valid WASMi runtime module from the binary.
Some features are no longer supported but are not required by the rWASM runtime:

- Module imports, global variables, and memory imports
- Global variables exports

## Structure

The rWASM binary format supports the following sections:

1. **Bytecode Section**: Replaces the function/code/entrypoint sections.
2. **Memory Section**: Replaces memory/data sections for all active/passive/declare section types.
3. **Function Section**: A temporary solution for the code section, planned for removal.
4. **Element Section**: Replaces the table/elem sections, also planned for removal.

### Bytecode Section

This section consolidates WASM's original function, code, and start sections.
It contains all instructions for the entire binary without any additional separators for functions.
Functions are recovered from the bytecode by reading the function section, which contains function lengths.
We inject the entrypoint function at the end, which is used to initialize all segments according to WASM constraints.

> **Note**: We plan to remove the function section and store the entrypoint at offset 0. To achieve this, we need to
> eliminate stack calls and implement indirect breaks. Although we have an implementation for this, it is not yet
> satisfactory, and we plan to migrate to a register-based IR before finalizing it.

### Memory Section

In WASM, memory and data sections are handled separately.
In rWASM, the Memory section defines memory bounds (lower and upper limits), and data sections,
which can be either active or passive, specify data to be mapped inside the memory.
Unlike WASM, rWASM eliminates the separate memory section,
modifies the corresponding instruction logic, and merges all data sections.

Here's an example of a WAT file that initializes memory with minimum and maximum memory bounds
(default allocated memory is one page, and the maximum possible allocated pages are two):

```wat
(module
  (memory 1 2)
)
```

To support this, we inject the `memory.grow` instruction into the entrypoint to initialize the default memory.
We also add a special preamble to all `memory.grow` instructions to perform upper bound checks.

Here is an example of the resulting entrypoint injection:

```wat
(module
  (func $__entrypoint
    i32.const $_init_pages
    memory.init
    drop)
)
```

According to WASM standards, a memory overflow causes `u32::MAX` to be placed on the stack.
For upper-bound checks, we can use the `memory.size` opcode.
Here is an example of such an injection:

```wat
(module
  (func $_func_uses_memory_grow
    (block
      local.get 1
      memory.size
      i32.add
      i32.const $_max_pages
      i32.gts
      drop
      i32.const 4294967295
      br 0
      memory.grow)
  )
)
```

These injections fully comply with WASM standards,
allowing us to support official WASM memory constraint checks for the memory section.

For the data section, the process is more complex because we need to support three different data section types:

- **Active**: Has a pre-defined compile-time offset.
- **Passive**: Can be initialized dynamically at runtime.

To address this, we merge all sections.
If the memory is active, we initialize it inside the entrypoint with re-mapped offsets.
Otherwise,
we remember the offset in a special mapping to adjust passive segments when the user calls `memory.init` manually.

Here is an example of an entrypoint injection for an active data segment:

```wat
(module
  (func $__entrypoint
    i32.const $_relative_offset
    i64.const $_data_offset
    i64.const $_data_length // or u64::MAX in case of overflow
    memory.init 0
    data.drop $segment_index+1
  )
)
```

We need to drop the data segment finally.
According to WASM standards, once the segment is initialized, it must be entirely removed from memory.
To simulate this behavior,
we use zero segments as a default and store special data segment flags to know which segments are still active.

For passive data segments, the logic is similar, but we must recalculate data segment offsets on the fly.

```wat
(module
  (func $_func_uses_memory_init
    // adjust length
    (block
      local.get 1
      local.get 3
      i32.add
      i32.const $_data_len
      i32.gts
      br_if_eqz 0
      i32.const 4294967295 // an error
      local.set 1
    )
    // adjust offset
    i32.const $_data_offset
    local.get 3
    i32.add
    local.set 2
    // do init
    memory.init $_segment_index+1
  )
)
```

The provided injections are examples and may vary based on specific requirements.

### Function Sections (Temporary)

The function section is a temporary measure used to store information about function lengths.
This section will be removed once we move the entrypoint function to the beginning of the module.

Currently, removing functions requires significant refactoring and modifications to our codebase, including:

1. Replacing all functions with breaks (e.g., `br` instructions).
2. Removing stack calls and using indirect breaks or tables.

We plan to migrate to a register-based VM in the future.

### Element Section (Temporary)

The element section uses the same translation logic as the memory/data sections
but operates with tables and elements instead of memory and data.

This section is also temporary and will eventually be replaced with memory operations.
Doing so can reduce the number of read/write operations and the size of our circuits.
The main challenge is managing memory securely to avoid mixing system and user memory spaces.
We aim to support original WASM binaries, regardless of how they are compiled,
without resorting to a custom WASM compilation target.

This approach is still under research.