## `i32_xor`

### **Description**

The `i32_xor` instruction **performs bitwise XOR (exclusive OR) operation on two 32-bit integers**. It pops two 32-bit integers from the stack, performs bitwise XOR, and pushes the result back onto the stack.

### **Behavior**

1. **Pops** the second operand (right-hand side) from the stack.
2. **Pops** the first operand (left-hand side) from the stack.
3. **Performs** bitwise XOR operation on the two values.
4. **Pushes** the result onto the stack.
5. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: Decreased by 1 (two values are popped, one is pushed).
- **Memory**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | lhs | rhs | sp ]
```

#### **After Execution:**

```
[ ... | result | sp ]
```

Where `result` is `lhs ^ rhs` (bitwise XOR).

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **binary operation** that consumes two stack values and produces one result.
- The operation is performed **bit by bit** - each bit in the result is 1 if the corresponding bits in the operands are different.
- **No traps** can occur during execution of this instruction.
- Equivalent to the expression `lhs ^ rhs` in high-level languages.
- The operation is **commutative**: `a ^ b = b ^ a`.
- The operation is **associative**: `(a ^ b) ^ c = a ^ (b ^ c)`.
- Identity element: `x ^ 0 = x`, Self-inverse: `x ^ x = 0`.
- Useful for bit manipulation and cryptographic operations.