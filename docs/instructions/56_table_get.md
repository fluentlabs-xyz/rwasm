## `table_get`

### **Description**

The `table_get` instruction **retrieves an element from a specified table** at a given index and pushes it onto the
stack.

### **Behavior**

1. **Pops** the index (`index`) from the stack.
2. **Retrieves** the element at `index` from the table identified by `table_idx`:
    - If `index` is **out of bounds**, execution **traps** (`TrapCode::TableOutOfBounds`).
    - If the retrieval is successful, the element is pushed onto the stack.
3. **Increments** the instruction pointer (`ip`) by `1`.

### **Registers and Memory Changes**

- **instruction pointer (`ip`)**: **Increased by `1`**.
- **stack pointer (`sp`)**: **Unchanged in count** (pops `index`, then pushes the retrieved value).
- **memory size (`ms`)**: **Unchanged**.
- **memory**: **Unchanged** (tables are separate from linear memory).
- **fuel counter (`fc`)**: **Unchanged**.

### **Stack Changes**

#### **Before Execution:**

```
[ ... | index | sp ]
```

#### **After Execution (Successful Retrieval):**

```
[ ... | value | sp ]
```

(`index` is replaced by the retrieved value.)

#### **After Execution (Table Out of Bounds - Trap):**

- **Execution halts due to a `TableOutOfBounds` trap.**

### **Operands**

- `table_idx` (integer): The index of the table from which to retrieve the value.

### **Notes**

- **Attempting to access an index beyond the table’s size results in a trap**.
- The instruction **does not modify `ms`** but operates on table storage.
- Used for **retrieving function references or other stored elements** from tables.