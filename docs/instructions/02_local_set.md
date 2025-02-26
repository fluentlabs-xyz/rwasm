## `local_set`

### **Description**

The `local_set` instruction updates the value of a local variable at the specified depth with a new value popped from
the stack.

### **Behavior**

1. Pops the top value from the stack.
2. Stores this value in the local variable at a depth of `local_depth` from the current stack position.
3. Increments the instruction pointer (`ip`) to move to the next instruction.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: Decremented by 1 due to the pop operation.

### **Stack Changes**

##### **Before Execution:**

```
[ ... | local_n | local_n-1 | ... | local_0 | new_value | sp ]
```

(`sp` points to the latest uninitialized stack position.)

##### **After Execution:**

```
[ ... | local_n | local_n-1 | ... | new_value | sp ]
```

(`new_value` is stored at `local_depth`, and `sp` moves back by one.)

### **Operands**

- `local_depth` (integer): specifies the depth of the local variable to be updated.

### **Notes**

- The local variable at `local_depth` is **overwritten** with the new value.
- The instruction reduces the stack size by one due to the pop operation.