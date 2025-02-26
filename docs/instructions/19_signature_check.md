## `signature_check`

### **Description**

The `signature_check` instruction **verifies that the last called function matches the expected signature**. If the
function signature does not match, execution traps.

### **Behavior**

1. **Retrieves** the last function signature from `last_signature`.
2. **Compares** it with the expected `signature_idx`:
    - If the signatures **do not match**, execution **traps** (`TrapCode::BadSignature`).
    - If the signatures **match** or no previous signature exists, execution continues.
3. **Increments** the instruction pointer (`ip`) by `1` to proceed.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**:
    - **Increased by `1`** if the signature check passes.
    - **Execution traps** if the signature is invalid.
- **stack pointer (`sp`)**: **Unchanged** (this instruction does not modify the stack).
- **memory**: **Unchanged** (this instruction does not interact with memory).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | function arguments | sp ]
```

#### **After Execution (Signature Valid - Execution Continues):**

```
[ ... | function arguments | sp ]
```

(`sp` remains unchanged, and `ip` increments by `1`.)

#### **After Execution (Signature Mismatch - Trap):**

- **Execution halts due to a `BadSignature` trap.**

### **Operands**

- `signature_idx` (integer): Expected function signature index.

### **Notes**

- This instruction is **used for indirect function calls** to ensure type safety.
- If the function signature **does not match**, execution **traps** immediately.
- If no previous signature exists, the instruction simply continues execution.