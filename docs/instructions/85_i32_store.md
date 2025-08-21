## `i32_store`

### **Description**

The `i32_store` instruction **stores a 32-bit integer to memory** at a specified address with an optional offset. It pops a value and a memory address from the stack, computes the effective address, and stores the 32-bit value at that location.

### **Behavior**

1. **Pops** the value to be stored from the stack.
2. **Pops** the memory address from the stack.
3. **Computes** the effective address by adding the offset to the popped address.
4. **Checks** if the effective address is within memory bounds.
5. **Stores** the 32-bit value to memory at the computed address.
6. **Increments** the instruction pointer (`ip`) by 1.
7. If the memory access is **out of bounds**, execution **traps** with `TrapCode::MemoryOutOfBounds`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1 (or execution halts on trap).
- **Stack Pointer (`sp`)**: Decreased by 2 (two values are popped).
- **Memory**: **Written** at the computed memory address.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | address | value | sp ]
```

#### **After Execution (Successful Store):**

```
[ ... | sp ]
```

#### **After Execution (Memory Out of Bounds - Trap):**

- **Execution halts** due to a `MemoryOutOfBounds` trap.

### **Operands**

- `offset` (AddressOffset): A constant offset added to the memory address before storing.

### **Notes**

- **Can trap** if the effective address is beyond the allocated memory range.
- The offset is applied **before** memory access and should be within valid bounds.
- **Modifies memory** by writing the value to the computed address.
- Stores exactly 4 bytes (32 bits) to memory.
- The value is stored as a 32-bit integer in **little-endian** byte order.
- Equivalent to storing an `int32_t` to memory at `address + offset`.
- Memory tracing may be performed if enabled in the configuration.