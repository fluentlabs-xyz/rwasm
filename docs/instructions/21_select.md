## `select`

### **Description**

The `select` instruction **chooses between two values on the stack** based on a condition. It pops a boolean condition
from the stack and returns one of the two preceding values, discarding the other.

### **Behavior**

1. **Pops** the top three values from the stack:
    - `e3`: Condition (boolean).
    - `e2`: Value if the condition is `false`.
    - `e1`: Value if the condition is `true`.
2. **Pushes** `e1` back onto the stack if `e3` is `true`, otherwise pushes `e2`.
3. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Decremented by `2`** (removes the condition and one discarded value).
- **memory**: **Unchanged** (this instruction does not interact with memory).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | value_if_true | value_if_false | condition | sp ]
```

#### **After Execution (Condition = `true`):**

```
[ ... | value_if_true | sp ]
```

#### **After Execution (Condition = `false`):**

```
[ ... | value_if_false | sp ]
```

(`sp` moves down by `2`, and only the selected value remains on the stack.)

### **Operands**

- **None** (operates on the top three stack values).

### **Notes**

- If `e3` is **nonzero (`true`)**, `e1` is selected.
- If `e3` is **zero (`false`)**, `e2` is selected.
- **Only one value remains on the stack after execution.**
- If the stack has fewer than three values before execution, **behavior is undefined**.