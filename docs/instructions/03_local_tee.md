## `local_tee`

### **Description**

The `local_tee` instruction duplicates the top value of the stack and stores it in a local variable at a specified
depth. Unlike `local_set`, it does not remove the value from the stack.

### **Behavior**

1. Reads the top value of the stack **without popping** it.
2. Stores this value in the local variable at a depth of `local_depth` from the current stack position.
3. Increments the instruction pointer (`ip`) to move to the next instruction.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`SP`)**: **Unchanged** (since no value is removed from the stack).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | local_n | local_n-1 | ... | local_0 | value | SP ]
```

(`SP` points to the latest uninitialized stack position.)

#### **After Execution:**

```
[ ... | local_n | local_n-1 | ... | value | local_0 | value | SP ]
```

(`value` is stored at `local_depth`, but remains on the stack.)

### **Operands**

- `local_depth` (integer): Specifies the depth of the local variable to be updated.

### **Notes**

- The local variable at `local_depth` is **overwritten** with the value from the top of the stack.
- The stack **remains unchanged** because `local_tee` only copies the value rather than popping it.
- Useful for preserving a value while updating a local variable.