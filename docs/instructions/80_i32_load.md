## `i32_load`

### **Description**

The `i32_load` instruction **loads a 32-bit integer from memory** at a specified address with an optional offset. It pops a memory address from the stack, adds the offset, loads a 32-bit integer from the computed address, and pushes the result onto the stack.

### **Behavior**

1. **Pops** the memory address from the stack.
2. **Computes** the effective address by adding the offset to the popped address.
3. **Checks** if the effective address is within memory bounds.
4. **Loads** a 32-bit integer from memory at the computed address.
5. **Pushes** the loaded value onto the stack.
6. **Increments** the instruction pointer (`ip`) by 1.
7. If the memory access is **out of bounds**, execution **traps** with `TrapCode::MemoryOutOfBounds`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1 (or execution halts on trap).
- **Stack Pointer (`sp`)**: **Unchanged** (one value is popped, one is pushed).
- **Memory**: **Read** at the computed memory address.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | address | sp ]
```

#### **After Execution (Successful Load):**

```
[ ... | loaded_value | sp ]
```

#### **After Execution (Memory Out of Bounds - Trap):**

- **Execution halts** due to a `MemoryOutOfBounds` trap.

### **Operands**

- `offset` (AddressOffset): A constant offset added to the memory address before loading.

### **Notes**

- **Can trap** if the effective address is beyond the allocated memory range.
- The offset is applied **before** memory access and should be within valid bounds.
- **Does not modify memory**, only reads from it.
- Loads exactly 4 bytes (32 bits) from memory.
- The loaded value is interpreted as a 32-bit signed integer.
- Memory is accessed in **little-endian** byte order.
- Equivalent to loading an `int32_t` from memory at `address + offset`.