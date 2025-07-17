## `i32_rem_u`

### **Description**

The `i32_rem_u` instruction **computes the unsigned remainder of dividing two 32-bit integers**. It pops two 32-bit integers from the stack, interprets them as unsigned integers, computes the remainder, and pushes the result back onto the stack.

### **Behavior**

1. **Pops** the second operand (divisor) from the stack.
2. **Pops** the first operand (dividend) from the stack.
3. **Checks** if the divisor is zero - if so, **traps** with `TrapCode::IntegerDivisionByZero`.
4. **Performs** unsigned remainder operation of the two values.
5. **Pushes** the result onto the stack.
6. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1 (or execution halts on trap).
- **Stack Pointer (`sp`)**: Decreased by 1 (two values are popped, one is pushed).
- **Memory**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | dividend | divisor | sp ]
```

#### **After Execution (Successful Operation):**

```
[ ... | result | sp ]
```

Where `result` is `(unsigned)dividend % (unsigned)divisor`.

#### **After Execution (Trap):**

- **Execution halts** due to division by zero trap.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **binary operation** that consumes two stack values and produces one result.
- Remainder is computed using **unsigned interpretation** of the 32-bit integers.
- **Can trap** on division by zero, but not on overflow.
- The result is always non-negative and less than the divisor.
- Equivalent to the expression `(uint32_t)dividend % (uint32_t)divisor` in high-level languages.
- The operation is **not commutative**: `a % b ≠ b % a` (in general).
- Satisfies the identity: `dividend = (dividend / divisor) * divisor + (dividend % divisor)`.