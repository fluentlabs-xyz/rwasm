## `select`

### **Description**

The `select` instruction **chooses between two values on the stack** based on a condition. It pops a boolean condition from the stack and returns one of the two preceding values, discarding the other.

### **Behavior**

1. **Evaluates** the top three values from the stack:
   - `e3`: Condition (boolean).
   - `e2`: Value if the condition is `false`.
   - `e1`: Value if the condition is `true`.
2. **Selects** `e1` if `e3` is `true`, otherwise selects `e2`.
3. **Replaces** the top three stack values with the selected value.
4. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Increased by `1`**.
- **Stack Pointer (`SP`)**: **Decremented by `2`** (removes the condition and one discarded value).
- **Memory**: **Unchanged** (this instruction does not interact with memory).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | value_if_true | value_if_false | condition | SP ]
```

#### **After Execution (Condition = `true`):**

```
[ ... | value_if_true | SP ]
```

#### **After Execution (Condition = `false`):**

```
[ ... | value_if_false | SP ]
```

(`SP` moves down by `2`, and only the selected value remains on the stack.)

### **Operands**

- **None** (operates on the top three stack values).

### **Notes**

- If the condition is **nonzero (`true`)**, the first value is selected.
- If the condition is **zero (`false`)**, the second value is selected.
- **Only one value remains on the stack after execution.**
- The operation is performed efficiently using the `eval_top3` method on the stack pointer.
- This instruction is commonly used to implement conditional value selection in WebAssembly.