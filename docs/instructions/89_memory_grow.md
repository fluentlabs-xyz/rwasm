## `memory_grow`

### **Description**

The `memory_grow` instruction **attempts to grow the memory by a specified number of pages**. It pops a delta value from the stack, attempts to grow the memory, and pushes the previous memory size onto the stack (or -1 if the growth failed).

### **Behavior**

1. **Pops** the delta (number of pages to grow) from the stack.
2. **Validates** the delta value to ensure it's a valid page count.
3. **Attempts** to grow the memory by the specified number of pages.
4. **Pushes** the previous memory size (on success) or -1 (on failure) onto the stack.
5. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: **Unchanged** (one value is popped, one is pushed).
- **Memory**: **May be grown** if the operation succeeds.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | delta | sp ]
```

#### **After Execution (Successful Growth):**

```
[ ... | previous_size | sp ]
```

#### **After Execution (Failed Growth):**

```
[ ... | -1 | sp ]
```

Where `previous_size` is the memory size before growth, and `delta` is the number of pages to add.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- **No traps** can occur during execution of this instruction.
- **May modify memory** by growing it, but never shrinks memory.
- The delta value is in **pages**, where each page is 64 KB (65,536 bytes).
- If the delta is invalid (e.g., would cause overflow), the instruction pushes -1 and fails.
- If memory growth fails due to system limits, the instruction pushes -1.
- On success, the previous memory size is returned, allowing the caller to know the new base address.
- Memory growth is **irreversible** - there is no corresponding shrink operation.
- Growing by 0 pages is valid and returns the current memory size.