## `consume_fuel` Instruction Specification

### **Description**

The `consume_fuel` instruction reduces the available execution fuel by a specified amount (`block_fuel`). If the
required fuel cannot be consumed, execution traps. This instruction is used to enforce computational limits within the
rWasm virtual machine.

### **Behavior**

1. Attempts to consume the specified `block_fuel` amount from the fuel counter (`fc`).
2. If there is **sufficient fuel**, it is deducted, and execution proceeds.
3. If there is **insufficient fuel**, execution traps (`RwasmError::TrapCode`).
4. If successful, the instruction pointer (`ip`) is incremented by `1` to continue execution.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`** if execution continues.
- **stack pointer (`sp`)**: **Unchanged** (this instruction does not interact with the stack).
- **fuel counter (`fc`)**: **Decreased** by `block_fuel` if fuel is available.
- **memory**: **Unchanged** (no read or write operations).

### **Stack Changes**

#### **Before Execution:**

```
[ ... | stack data | sp ]
```

#### **After Execution (Fuel Available):**

```
[ ... | stack data | sp ]
```

(`sp` remains unchanged, `ip` is incremented by `1`, and `fc` decreases by `block_fuel`.)

#### **After Execution (Fuel Exhausted - Trap):**

- **Execution halts due to a trap.**

### **Operands**

- `block_fuel` (unsigned integer): Specifies the amount of fuel to consume.

### **Notes**

- `consume_fuel` is primarily used for **resource metering**, ensuring that execution does not exceed predefined
  computational limits.
- If the required fuel cannot be deducted, the VM **traps** to prevent excessive resource usage.
- This instruction does **not** modify the stack but controls execution flow via fuel consumption.