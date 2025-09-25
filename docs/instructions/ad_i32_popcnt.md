## `i32_popcnt`

### **Description**

The `i32_popcnt` instruction **counts the number of set bits** (1-bits) in a 32-bit integer. It pops a 32-bit integer from the stack and pushes the count of bits that are set to 1.

### **Behavior**

1. **Pops** a 32-bit integer from the stack.
2. **Counts** the number of bits that are set to 1 in the value.
3. **Pushes** the count as a 32-bit integer.
4. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: **Unchanged** (one value is popped, one is pushed).
- **Memory**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | value | sp ]
```

#### **After Execution:**

```
[ ... | count | sp ]
```

Where `count` is the number of set bits in `value`.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **unary operation** that consumes one stack value and produces one result.
- If the input value is 0, the result is 0 (no bits are set).
- If the input value is 0xFFFFFFFF, the result is 32 (all bits are set).
- **No traps** can occur during execution of this instruction.
- Also known as "population count" or "Hamming weight" operation in other contexts.