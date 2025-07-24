## `global_get`

### **Description**

The `global_get` instruction **retrieves the value of a global variable** and pushes it onto the stack. It reads the global variable at the specified index and places its value on the stack.

### **Behavior**

1. **Retrieves** the value of the global variable at the specified index.
2. **If the global variable exists**, its value is obtained.
3. **If the global variable doesn't exist**, a default value is used.
4. **Pushes** the global variable value onto the stack.
5. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Increased by `1`**.
- **Stack Pointer (`SP`)**: **Incremented by `1`** (pushes the global value).
- **Memory**: **Unchanged** (this instruction does not interact with memory).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | stack data | SP ]
```

#### **After Execution:**

```
[ ... | stack data | global_value | SP ]
```

(`SP` moves up by `1`, and the global variable value is added to the stack.)

### **Operands**

- `global_idx` (GlobalIdx): The index of the global variable to retrieve.

### **Notes**

- If the global variable doesn't exist at the specified index, a default value is returned.
- The global variable value is retrieved from the store's global variables collection.
- This instruction is used to access global state in WebAssembly modules.
- The operation is read-only and does not modify the global variable.