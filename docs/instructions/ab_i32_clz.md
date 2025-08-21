## `i32_clz`

### **Description**

The `i32_clz` instruction **counts the number of leading zero bits** in a 32-bit integer. It pops a 32-bit integer from the stack and pushes the count of consecutive zero bits starting from the most significant bit.

### **Behavior**

1. **Pops** a 32-bit integer from the stack.
2. **Counts** the number of consecutive zero bits starting from the most significant bit (bit 31).
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

Where `count` is the number of leading zero bits in `value`.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **unary operation** that consumes one stack value and produces one result.
- If the input value is 0, the result is 32 (all bits are leading zeros).
- If the input value has the most significant bit set (0x80000000), the result is 0.
- **No traps** can occur during execution of this instruction.
- Also known as "count leading zeros" operation in other architectures.