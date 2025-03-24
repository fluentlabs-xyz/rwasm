## `i32_load`

### **Description**

The `i32_load` instruction **loads a 32-bit integer from memory** at a specified offset and pushes it onto the stack.

### **Behavior**

1. **Pops** the memory address from the stack.
2. **Computes** the effective address by adding `offset` to the popped address.
3. **Loads** a 32-bit integer from memory at the computed address.
4. **Pushes** the loaded value onto the stack.
5. If the memory access is **out of bounds**, execution **traps** (`TrapCode::MemoryOutOfBounds`).

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Unchanged** (unless a trap occurs).
- **stack pointer (`sp`)**: **Unchanged** in count (one value is popped, one is pushed).
- **memory**: **Read** at the computed memory address.
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | address | sp ]
```

#### **After Execution (Successful Load):**

```
[ ... | loaded_value | sp ]
```

(The address is replaced with the loaded 32-bit integer.)

#### **After Execution (Memory Out of Bounds - Trap):**

- **Execution halts due to a `MemoryOutOfBounds` trap.**

### **Operands**

- `offset` (integer): A constant offset added to the memory address before loading.

### **Notes**

- **Accessing memory beyond its allocated range results in a trap**.
- The **offset is applied before memory access** and should be within valid bounds.
- **Does not modify memory**, only reads from it.