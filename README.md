rWASM (reduced-WebAssembly)
===========================

## rWASM

rWASM (reduced WebAssembly) is an EIP-3540 compatible binary IR (intermediary representation) of WASM (WebAssembly) that
is used to simplify the execution process of WASM binaries by keeping 100% compatibility with original WASM features.
It achieves ZK-friendliness by having more flattened binary structure and simplified instruction set.
IMPORTANT: rWASM is a trusted execution runtime, don't run it w/o validation.
It's safe to translate WASM to rWASM and
execute since it injects all validations inside entrypoint.

## Technology

rWASM is based on WASMi's IR developed by [Parity Tech](https://github.com/wasmi-labs/wasmi) and now under Robin
Freyler's ownership.
We decided to choose the WASMi virtual machine because its IR is fully identical to the original WASM's has an anxiety
disorder position.
For rWASM, we follow the same principle.
Additionally, we don't modify WASMi's IR; instead, we only modify the binary representation to achieve ZK-friendliness.

Here is a list of differences:

1. Deterministic function order based on their position in the codebase
2. Block/Loop are replaced with Br-family instructions
3. Break instructions are redesigned to support PC offsets instead of depth-level
4. Most of the sections are removed to simplify binary verification
5. The new memory segment section that implements all WASM memory standards in one place
6. Removed global variables section
7. Type mapping is not required anymore since code is fully validated
8. Special entrypoint function that in its all segments

The new binary representation produces 100% valid WASMi's runtime module from binary.
There are several features that are not supported anymore, but not required by rWASM runtime:

- module, global variables and memory imports
- global variables exports

## Structure

rWASM binary format supports the following sections:

1. bytecode section (it replaces function/code/entrypoint sections)
2. memory section (it replaces memory/data for all active/passive/declare section types)
3. function section (temporary solution for a code section, will be removed)
4. element section (it replaces table/elem sections, will be removed)

### Bytecode section

This section replaces WASM's original function/code/start sections.

Bytecode contains all instructions for the entire binary w/o any additional separators for functions.
Functions can be recovered from a bytecode by reading function section that contains function lengths.
We inject entrypoint function in the end.
Entrypoint is used to initialize all segments according to WASM constraints.

P.S: we're planning to remove the function section and store entrypoint at offset 0.
To achieve this, we need to remove stack call and implement indirect breaks.
We have implementation of this, but it's not good enough, and we're planning to migrate to register-based IR before
implementing this.

### Memory section

WASM has memory and data sections.
The Memory section is used to define memory bounds (lower and upper limits).
Data sections can be active/passive and are used to define data to be mapped inside data.
Comparing to WASM, we remove a memory section, modify the corresponding instruction logic and merge all data sections.

Here is an example of WAT file that initializes memory with min/max memory bounds (default allocated memory is one page
and max possible allocated pages are 2):

```wat
(module
  (memory 1 2)
)
```

To support this we inject `memory.grow` instruction into entrypoint that inits default memory and also inject special
preamble into all `memory.grow` instruction to do upper bound checks.

Here is an example of the resulting entrypoint injection:

```wat
(module
  (func $__entrypoint
    i32.const $_init_pages
    memory.init
    drop)
)
```

According to the WASM standards, memory overflow causes `u32::MAX` on the stack.
For upper-bound checks we can do a memory overflow check using `memory.size` opcode.
Here is an example of such injection:

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

These injections fully match WASM standards, and this is how we can support official WASM memory constraint checks for
a memory section.

For data section, it's a bit more complicated because we have to support three different data section types:

- `active` - has a pre-defined compile-time offset
- `passive` - can be initialized dynamically in runtime

To solve this problem, we merge all sections.
If memory active then we inits it inside entrypoint with
re-mapped offsets otherwise remember offset in a special mapping (we need this to adjust passive segments when user
call `memory.init` manually).

Here is an example of entrypoint injection for active data segment:

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

We need to do a final trick with a data segment drop,
because, according to WASM standards, once the segment is initialized, then
it must be entirely removed from memory.
To simulate the same behavior,
we use zero segments as a default and store special data segments flags to know what segment
is still alive.

For passive data segments, the logic is almost the same, but we must recalculate data segment offsets on flight.

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

Provided injections upper are examples and can be different.

### Function sections (temporary)

This section is temporary and is used to store information about function length.
We're planning to remove this section once we move the entrypoint function into the beginning of the module.

We can't do this right now, because removing functions requires a lot of refactoring and modifications inside out
codebase:

1. Replace all functions with breaks (like `br` instructions)
2. Remove stack call and use indirect breaks or tables

We're planning to migrate to the register-based VM.

### Element section (temporary)

This section uses the same translation logic as memory/data sections.
The only different is that it operates with tables and elements instead of memory and data.

Element section is also temporary.
We don't have to keep this section because we can replace it with memory operations.
It can reduce the number of RW ops and size of our circuits.
The biggest challenge is how to manage memory securely in this case and avoid mixing system and user memory spaces.
We don't want to go with custom WASM compilation target and want to support original WASM binaries (now matter how they
compiled).

This is still under research.