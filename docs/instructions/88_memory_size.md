## `memory_size`

### **Description**

The `memory_size` instruction **returns the current size of the memory in pages**. It pushes the current memory size (in 64 KB pages) onto the stack without modifying memory.

### **Behavior**

1. **Queries** the current memory size in pages from the global memory.
2. **Pushes** the memory size as a 32-bit unsigned integer onto the stack.
3. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: Increased by 1 (one value is pushed).
- **Memory**: **Unchanged** (only queried, not modified).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | sp ]
```

#### **After Execution:**

```
[ ... | memory_size | sp ]
```

Where `memory_size` is the current number of 64 KB pages in memory.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- **No traps** can occur during execution of this instruction.
- **Does not modify memory**, only queries its current size.
- The returned size is in **pages**, where each page is 64 KB (65,536 bytes).
- The result is always a non-negative integer.
- The actual memory size in bytes is `memory_size * 65536`.
- This instruction is commonly used before `memory_grow` to check available memory.
- Memory size can change during program execution due to `memory_grow` instructions.