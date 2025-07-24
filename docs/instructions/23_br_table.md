## `br_table`

### **Description**

The `br_table` instruction performs a **jump table operation** that branches to different locations based on an index value. It pops an index from the stack and uses it to select the target from a table of branch targets.

### **Behavior**

1. **Pops** an index value from the stack.
2. **Normalizes** the index to ensure it's within the target table bounds.
3. **Calculates** the target instruction address using the normalized index.
4. **Modifies** the instruction pointer (`ip`) to jump to the selected target.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Modified** to jump to the selected target address.
- **Stack Pointer (`SP`)**: **Decremented by `1`** (pops the index value).
- **Memory**: **Unchanged** (this instruction does not interact with memory).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | stack data | index | SP ]
```

#### **After Execution:**

```
[ ... | stack data | SP ]
```

(`SP` moves down by `1`, and execution continues at the selected target.)

### **Operands**

- `targets` (BranchTableTargets): The number of branch targets available in the table.

### **Notes**

- The index is clamped to the valid range: if the index exceeds the table size, it's normalized to the maximum valid index.
- The instruction pointer is updated to: `ip + (2 * normalized_index) + 1`.
- This instruction is commonly used to implement `switch` statements in compiled WebAssembly code.
- The target calculation accounts for the fact that each target takes 2 instruction slots.