## `memory_copy`

### **Description**

The `memory_copy` instruction **copies a specified number of bytes from one memory location to another**. It reads the
destination offset, source offset, and length from the stack and performs a memory copy operation.

### **Behavior**

1. **Pops** three values from the stack:
    - `d`: The **destination offset** in memory.
    - `s`: The **source offset** in memory.
    - `n`: The **number of bytes** to copy.
2. **Converts** `s`, `d`, and `n` to `usize`.
3. **If fuel metering is enabled**, it:
    - **Consumes fuel (`fc`)** based on `n` bytes.
    - If fuel is insufficient, execution **traps**.
4. **Validates** the memory range:
    - If the source or destination range **exceeds available memory**, execution **traps** (
      `TrapCode::MemoryOutOfBounds`).
5. **Performs** the memory copy from `[s..s+n]` to `[d..d+n]`.
6. **Logs** memory changes if tracing is enabled.
7. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Decremented by `3`** (pops `d`, `s`, and `n`).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Modified** at the destination range.
- **fuel counter (`fc`)**:
    - **Decreased** based on `n` bytes if fuel metering is enabled.
    - **Unchanged** if fuel metering is disabled.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | destination | source | length | sp ]
```

#### **After Execution (Successful Copy):**

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

- **Copying outside allocated memory results in a trap**.
- If `n` is `0`, **no memory is modified**.
- If the **source and destination regions overlap**, the copy is performed **correctly** using `copy_within`.
- The instruction **does not modify `ms`** but writes directly to memory.