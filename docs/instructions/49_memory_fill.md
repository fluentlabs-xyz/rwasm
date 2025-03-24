## `memory_fill`

### **Description**

The `memory_fill` instruction **fills a specified memory region with a given byte value**. It reads the destination
address, byte value, and length from the stack and writes the byte value across the specified memory range.

### **Behavior**

1. **Pops** three values from the stack:
    - `d`: The **destination offset** in memory.
    - `val`: The **byte value** to write.
    - `n`: The **number of bytes** to fill.
2. **Converts** `d` and `n` to `usize` and `val` to `u8`.
3. **If fuel metering is enabled**, it:
    - **Consumes fuel (`fc`)** based on `n` bytes.
    - If fuel is insufficient, execution **traps**.
4. **Validates** the memory range:
    - If the offset and length **exceed available memory**, execution **traps** (`TrapCode::MemoryOutOfBounds`).
5. **Fills** the memory at `[d..d+n]` with `val`.
6. **Logs** memory changes if tracing is enabled.
7. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Decremented by `3`** (pops `d`, `val`, and `n`).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Modified** in the specified range.
- **fuel counter (`fc`)**:
    - **Decreased** based on `n` bytes if fuel metering is enabled.
    - **Unchanged** if fuel metering is disabled.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | destination | byte_value | length | sp ]
```

#### **After Execution (Successful Fill):**

```
[ ... | sp ]
```

(`sp` moves down by `3`, as the parameters are removed.)

#### **After Execution (Memory Out of Bounds - Trap):**

- **Execution halts due to a `MemoryOutOfBounds` trap.**

#### **After Execution (Insufficient Fuel - Trap):**

- **Execution halts due to a fuel exhaustion trap.**

### **Operands**

- **None** (operates based on the top three stack values).

### **Notes**

- **Writing outside allocated memory results in a trap**.
- If `n` is `0`, **no memory is modified**.
- If fuel metering is enabled and there is **not enough fuel to write `n` bytes, execution traps**.
- The instruction **does not modify `ms`** but writes directly to memory.