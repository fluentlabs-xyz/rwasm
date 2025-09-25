## `trap`

### **Description**

The `trap` instruction immediately **triggers a trap** when executed using the specified trap code. This instruction is used to deliberately cause a controlled program termination with a specific error condition.

### **Behavior**

1. **Triggers a trap** with the specified `trap_code` parameter.
2. **Execution halts** immediately and does not continue beyond this point.
3. **Returns the specified trap code** to the caller.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Execution halts** due to the trap.
- **Stack Pointer (`SP`)**: **Unchanged** (as no stack operations occur before the trap).
- **Memory**: **Unchanged** (as no memory read/write occurs).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | stack data | SP ]
```

#### **After Execution:**

- **Execution is halted due to a trap.**
- **No stack changes occur.**

### **Operands**

- `trap_code` (TrapCode): The specific trap code to trigger.

### **Notes**

- This instruction is used for controlled error handling and program termination.
- The trap code specifies the exact type of error condition that occurred.
- Any attempt to execute `trap` results in an **immediate trap** with the specified code.
- This instruction is commonly used to signal specific error conditions in compiled WebAssembly code.