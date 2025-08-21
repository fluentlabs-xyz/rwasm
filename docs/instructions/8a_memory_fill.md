## `memory_fill`

### **Description**

The `memory_fill` instruction **fills a specified memory region with a given byte value**. It reads the destination address, byte value, and length from the stack and writes the byte value across the specified memory range.

### **Behavior**

1. **Pops** three values from the stack:
   - `d`: The **destination offset** in memory.
   - `val`: The **byte value** to write.
   - `n`: The **number of bytes** to fill.
2. **Converts** `d` and `n` to `usize` and `val` to `u8`.
3. **Validates** the memory range:
   - If the offset and length **exceed available memory**, execution **traps** (`TrapCode::MemoryOutOfBounds`).
4. **Fills** the memory at `[d..d+n]` with `val`.
5. **Logs** memory changes if tracing is enabled.
6. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Increased by `1`**.
- **Stack Pointer (`SP`)**: **Decremented by `3`** (pops `d`, `val`, and `n`).
- **Memory**: **Modified** in the specified range.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | destination | byte_value | length | SP ]
```

#### **After Execution (Successful Fill):**

```
[ ... | SP ]
```

(`SP` moves down by `3`, as the parameters are removed.)

#### **After Execution (Memory Out of Bounds - Trap):**

- **Execution halts due to a `MemoryOutOfBounds` trap.**

### **Operands**

- **None** (operates based on the top three stack values).

### **Notes**

- **Writing outside allocated memory results in a trap**.
- If `n` is `0`, **no memory is modified**.
- The instruction **does not modify memory size** but writes directly to memory.
- Memory tracing is performed if the tracing feature is enabled.
- The byte value is extracted from the stack value and truncated to `u8`.