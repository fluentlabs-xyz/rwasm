## `unreachable`

### **Description**

The `unreachable` instruction immediately **traps** when executed. It is used to indicate that an invalid or undefined
execution path has been reached. This typically results in program termination or an exception being raised.

### **Behavior**

1. Causes a **trap** (`UnreachableCodeReached`) when executed.
2. Execution **does not continue** beyond this point.
3. No registers, memory, or stack operations are performed other than triggering the trap.

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

- **None** (This instruction does not take any operands).

### **Notes**

- `unreachable` is used for debugging, error handling, and enforcing correctness in execution paths.
- Any attempt to execute `unreachable` results in an **immediate trap**.
- This instruction is commonly used in generated WebAssembly code for scenarios that should never happen (e.g., missing
  case handling in a `match` statement).