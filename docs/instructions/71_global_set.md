## `global_set`

### **Description**

The `global_set` instruction **sets the value of a global variable** using a value from the stack. It pops a value from the stack and stores it in the global variable at the specified index.

### **Behavior**

1. **Pops** a value from the stack.
2. **Stores** the popped value in the global variable at the specified index.
3. **Updates** the store's global variables collection.
4. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Increased by `1`**.
- **Stack Pointer (`SP`)**: **Decremented by `1`** (pops the new value).
- **Memory**: **Unchanged** (this instruction does not interact with memory).
- **Global Variables**: **Modified** (updates the specified global variable).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | stack data | new_value | SP ]
```

#### **After Execution:**

```
[ ... | stack data | SP ]
```

(`SP` moves down by `1`, and the value is stored in the global variable.)

### **Operands**

- `global_idx` (GlobalIdx): The index of the global variable to set.

### **Notes**

- The global variable is created or updated in the store's global variables collection.
- If the global variable doesn't exist at the specified index, it will be created.
- This instruction is used to modify global state in WebAssembly modules.
- The operation consumes the top stack value and stores it persistently in the global variable.