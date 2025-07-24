## `consume_fuel_stack`

### **Description**

The `consume_fuel_stack` instruction **consumes fuel from the store** based on a value popped from the stack. This instruction is used for fine-grained fuel metering where the amount of fuel to consume is determined dynamically at runtime.

### **Behavior**

1. **Pops** a fuel amount value from the stack.
2. **If fuel metering is enabled**, it:
   - **Consumes fuel** from the store based on the popped value.
   - **Traps** if insufficient fuel is available.
3. **If fuel metering is disabled**, the instruction has no effect.
4. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: **Increased by `1`**.
- **Stack Pointer (`SP`)**: **Decremented by `1`** (pops the fuel amount).
- **Memory**: **Unchanged** (this instruction does not interact with memory).
- **Fuel Counter (`fc`)**: **Decreased** by the popped amount if fuel metering is enabled.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | stack data | fuel_amount | SP ]
```

#### **After Execution (Successful):**

```
[ ... | stack data | SP ]
```

(`SP` moves down by `1`, and the fuel amount is consumed.)

#### **After Execution (Insufficient Fuel - Trap):**

- **Execution halts due to a fuel exhaustion trap.**

### **Operands**

- **None** (operates on the top stack value).

### **Notes**

- The fuel amount is converted from the stack value to a `u32` before consumption.
- If fuel metering is disabled in the store configuration, this instruction effectively becomes a no-op that only pops the stack.
- This instruction provides dynamic fuel consumption based on runtime values.
- Fuel exhaustion results in an immediate trap, halting execution.