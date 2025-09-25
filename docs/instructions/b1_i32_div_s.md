## `i32_div_s`

### **Description**

The `i32_div_s` instruction **divides two 32-bit integers using signed division**. It pops two 32-bit integers from the stack, interprets them as signed integers, performs division, and pushes the result back onto the stack.

### **Behavior**

1. **Pops** the second operand (divisor) from the stack.
2. **Pops** the first operand (dividend) from the stack.
3. **Checks** if the divisor is zero - if so, **traps** with `TrapCode::IntegerDivisionByZero`.
4. **Checks** for signed overflow (INT32_MIN / -1) - if so, **traps** with `TrapCode::IntegerOverflow`.
5. **Performs** signed division of the two values.
6. **Pushes** the result onto the stack.
7. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1 (or execution halts on trap).
- **Stack Pointer (`sp`)**: Decreased by 1 (two values are popped, one is pushed).
- **Memory**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | dividend | divisor | sp ]
```

#### **After Execution (Successful Division):**

```
[ ... | result | sp ]
```

Where `result` is `(signed)dividend / (signed)divisor`.

#### **After Execution (Trap):**

- **Execution halts** due to division by zero or integer overflow trap.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **binary operation** that consumes two stack values and produces one result.
- Division is performed using **signed interpretation** of the 32-bit integers.
- **Can trap** on division by zero or integer overflow (INT32_MIN / -1).
- The result is truncated towards zero (same as C-style signed division).
- Equivalent to the expression `(int32_t)dividend / (int32_t)divisor` in high-level languages.
- The operation is **not commutative**: `a / b ≠ b / a` (in general).