## `local_get`

### **Description**

The `local_get` instruction retrieves the value of a local variable at the specified depth from the current stack
position and pushes it onto the stack.

### **Behavior**

1. Reads the local variable at a depth of `local_depth` from the top of the initialized stack.
2. Pushes the retrieved value onto the stack.
3. Increments the instruction pointer (`ip`) to move to the next instruction.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: Incremented by 1 due to the push operation.

### **Stack Changes**

#### Before Execution:

```
[ ... | local_n | local_n-1 | ... | local_0 | sp ]
```

(`sp` points to the latest uninitialized stack position.)

#### After Execution:

```
[ ... | local_n | local_n-1 | ... | local_0 | value | sp ]
```

(`value` is the retrieved local variable at `local_depth`.)

### **Operands**

- `local_depth` (integer): specifies the depth of the local variable to retrieve from the current stack.

### **Notes**

- The instruction does not modify the value of the local variable being accessed.
- The operation is effectively duplicating the local variable onto the stack.