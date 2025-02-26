## `memory_grow`

### **Description**

The `memory_grow` instruction **attempts to increase the size of linear memory** by a given number of pages. It consumes
fuel if a fuel limit is set and pushes the previous memory size onto the stack. If the memory growth fails, it pushes
`u32::MAX`.

### **Behavior**

1. **Pops** the number of pages to grow (`delta`) from the stack.
2. **Validates** `delta`:
    - If `delta` is invalid (too large to represent in pages), pushes `u32::MAX`, increments `ip` by `1`, and **returns
      **.
3. **If fuel metering is enabled**, it:
    - Converts `delta` to bytes.
    - **Consumes** fuel (`fc`) based on the number of bytes required.
    - If fuel is insufficient, execution **traps**.
4. **Attempts** to grow memory by `delta` pages:
    - If successful, **pushes the previous memory size (in pages)** onto the stack.
    - If growth fails, **pushes `u32::MAX`**.
5. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Unchanged in count** (pops `delta`, then pushes the old memory size or `u32::MAX`).
- **memory size (`ms`)**: **Updated** if memory growth succeeds.
- **memory**: **Expanded** if growth is successful.
- **fuel counter (`fc`)**:
    - **Decreased** based on the number of bytes added.
    - **Unchanged** if no fuel metering is enabled.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | delta | sp ]
```

#### **After Execution (Successful Growth):**

```
[ ... | previous_memory_size | sp ]
```

(`sp` remains the same, but the previous memory size is stored.)

#### **After Execution (Failed Growth or Invalid `delta`):**

```
[ ... | u32::MAX | sp ]
```

(`sp` remains unchanged, and `u32::MAX` is pushed to indicate failure.)

#### **After Execution (Insufficient Fuel - Trap):**

- **Execution halts due to a fuel exhaustion trap.**

### **Operands**

- **None** (operates based on the top stack value).

### **Notes**

- Memory growth is **measured in pages** (1 page = 64 KiB).
- If `delta` is too large to be represented in pages, the instruction **fails gracefully** by pushing `u32::MAX`.
- If memory growth fails (e.g., due to exceeding limits), `u32::MAX` is **pushed to indicate failure**.
- Fuel is **only consumed if a fuel limit is configured**.