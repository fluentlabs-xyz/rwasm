## `memory_size` Instruction Specification

### **Description**

The `memory_size` instruction **retrieves the current size of linear memory** in **WebAssembly pages** (each page is 64
KiB) and pushes it onto the stack.

### **Behavior**

1. **Fetches** the current memory size from the `ms` register (measured in pages).
2. **Pushes** the memory size onto the stack.
3. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Incremented by `1`** (stores the memory size).
- **memory size (`ms`)**: **Read-only** (retrieves the current memory size).
- **memory**: **Unchanged** (this instruction does not modify memory).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | sp ]
```

#### **After Execution:**

```
[ ... | memory_size (in pages) | sp ]
```

(The stack pointer moves up by `1`, and the memory size is pushed onto the stack.)

### **Operands**

- **None** (retrieves the memory size directly from `ms`).

### **Notes**

- Memory size is measured in **WebAssembly pages** (1 page = 64 KiB).
- This instruction **does not modify memory**, only queries its current size.
- Used for **checking available memory** before performing operations that might require additional allocation.