## `i32_rotr`

### **Description**

The `i32_rotr` instruction **performs right rotation operation on a 32-bit integer**. It pops two 32-bit integers from the stack (value and rotation amount), rotates the value right by the specified number of bits, and pushes the result back onto the stack.

### **Behavior**

1. **Pops** the rotation amount from the stack.
2. **Pops** the value to be rotated from the stack.
3. **Masks** the rotation amount to 5 bits (rotation_amount & 0x1F) to ensure it's in range [0, 31].
4. **Performs** right rotation operation on the value.
5. **Pushes** the result onto the stack.
6. **Increments** the instruction pointer (`ip`) by 1.

### **Registers and Memory Changes**

- **Instruction Pointer (`ip`)**: Increased by 1.
- **Stack Pointer (`sp`)**: Decreased by 1 (two values are popped, one is pushed).
- **Memory**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | value | rotation_amount | sp ]
```

#### **After Execution:**

```
[ ... | result | sp ]
```

Where `result` is `value` rotated right by `rotation_amount & 0x1F` bits.

### **Operands**

- **None** (This instruction does not take any operands).

### **Notes**

- This is a **binary operation** that consumes two stack values and produces one result.
- The rotation amount is automatically **masked to 5 bits** to prevent undefined behavior.
- **No traps** can occur during execution of this instruction.
- **Rotation** differs from shifting - bits that are rotated out of one end are rotated into the other end.
- **No information is lost** during rotation (unlike shifting).
- Right rotation by n bits is equivalent to: `(value >> n) | (value << (32 - n))`.
- Rotation is **reversible** - `rotr(rotr(x, n), 32-n) = x`.
- Rotation by 0 or any multiple of 32 returns the original value unchanged.
- `rotr(x, n)` is equivalent to `rotl(x, 32-n)`.