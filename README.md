# rWasm (Reduced WebAssembly)

[![codecov](https://codecov.io/gh/fluentlabs-xyz/rwasm/graph/badge.svg?token=9T2PLQQW4L)](https://codecov.io/gh/fluentlabs-xyz/rwasm)

**rWasm** is a ZK-friendly binary intermediate representation (IR) of WebAssembly (Wasm), designed for fast execution
and efficient zero-knowledge proof generation. It preserves full semantic compatibility with Wasm while removing
non-deterministic and hard-to-prove elements, making it the **only WebAssembly runtime specifically optimized for
blockchain and zero-knowledge applications**.

## Key Benefits

### **ZK-Optimized Design**

- **Flattened control flow**: Eliminates complex branching structures by converting all control flow to simple `br`,
  `br_if`, and `br_table` instructions
- **No relative jump targets**: Uses PC-relative branch targets instead of relative depth, eliminating a major
  ZK-proving bottleneck
- **Simplified validation**: No post-decode validation required, making it immediately executable and ZK-friendly

### **Superior Performance**

- **6x faster module loading** compared to Wasmi in no-cache scenarios (common in blockchain environments)
- **484ns cached execution** vs Wasmi's 341ns (competitive performance)
- **879ns no-cache execution** vs Wasmi's 5379ns (significant advantage)
- **Optimized for blockchain**: Fast startup performance critical for smart contract execution

### **Blockchain-Native Features**

- **Deterministic execution**: Consistent behavior across all network nodes
- **Fuel-based gas metering**: Built-in resource management system
- **Multiple execution backends**: rWasm, Wasmtime, and Wasmi integration

### **Full WebAssembly Compatibility**

- **Semantic preservation**: Every Wasm feature is either preserved or safely substituted
- **Standard compilation**: Compile from any language that targets WebAssembly
- **Easy migration**: Drop-in replacement for existing Wasm applications

## How rWasm Compares to Competitors

| Feature                     | rWasm         | Wasmtime | Wasmi    | WAMR     | V8 Wasm  | WasmEdge | Wasmer   |
|-----------------------------|---------------|----------|----------|----------|----------|----------|----------|
| **ZK-Friendly**             | ✅             | ❌        | ❌        | ❌        | ❌        | ❌        | ❌        |
| **Fast Module Loading**     | ✅             | ✅        | ✅        | ✅        | ✅        | ✅        | ✅        |
| **Blockchain Optimized**    | ✅             | ❌        | ⚠️       | ❌        | ❌        | ❌        | ❌        |
| **No-Cache Performance**    | **6x faster** | Standard | Baseline | Standard | Standard | Standard | Standard |
| **Deterministic Execution** | ✅             | ⚠️       | ✅        | ⚠️       | ❌        | ⚠️       | ⚠️       |
| **Gas Metering**            | ✅             | ❌        | ❌        | ❌        | ❌        | ❌        | ❌        |

## Performance Benchmarks

*Benchmark: fib(47) on Apple M3 MAX*

| Engine     | Cached Execution | No-Cache Execution | Use Case            |
|------------|------------------|--------------------|---------------------|
| **Native** | 12ns             | 12ns               | Baseline            |
| **rWasm**  | 484ns            | **879ns**          | **ZK/Blockchain**   |
| **Wasmi**  | 341ns            | 5379ns             | Embedded/Blockchain |

**Key Insights:**

- **No-cache scenarios** (common in blockchain): rWasm is **6x faster** than Wasmi
- **ZK applications**: Only rWasm provides flattened control flow suitable for ZK proofs
- **Smart contracts**: Fast module loading crucial for per-transaction execution

## Motivation

WebAssembly is an attractive binary format thanks to its structured control flow, clear memory model, and rich ecosystem
support.
However, its current binary design introduces complexity for ZK proving:

### **Standard Wasm Challenges:**

- **Relative jump targets** → Complex to trace in ZK circuits
- **Indirect function calls** → Non-deterministic execution paths
- **Type-table indirection** → Requires complex validation
- **Imports/exports** → Dynamic semantics unsuitable for ZK

### **rWasm Solutions:**

- **Flattened control flow** → Simple, linear execution trace
- **Embedded metadata** → No external dependencies
- **No validation overhead** → Immediately executable
- **Self-contained modules** → Deterministic execution

## Core Design Principles

### **Deterministic Layout**

- Functions are inlined into a flat bytecode section
- All branch targets are PC-relative
- Control structures (`block`, `loop`, `if`) are desugared into explicit `br` sequences

### **No Type Mapping**

- Function types are validated at rWasm compile-time and inlined
- No external type section needed
- Module is immediately executable without prior type resolution

### **No Dynamic Imports**

- rWasm is self-contained: no `import` or `export` sections required
- All external dependencies must be pre-resolved

## Binary Structure

| Section  | Purpose                             |
|----------|-------------------------------------|
| Bytecode | Flat instruction stream             |
| Memory   | Merged memory and data segments     |
| Element  | Optional: Table segment placeholder |

## Control Flow Rewriting

All structured blocks are rewritten into `br`, `br_if`, and `br_table` with relative PC offsets:

**Standard Wasm:**

```wasm
(block $label
  ...code...
  br $label
)
```

**rWasm:**

```wasm
...code...
br @relative_pc_offset
```

## Advanced Features

### **Multiple Execution Backends**

- **rWasm**: Custom interpreter optimized for flattened format
- **Wasmtime**: Integration with Bytecode Alliance runtime
- **Wasmi**: Lightweight interpreter integration

### **Blockchain-Specific Optimizations**

- **Fuel-based execution**: Gas metering system for resource management
- **Syscall support**: Integration with host environment functions
- **Resumable execution**: Support for interrupting and resuming execution

### **Performance Enhancements**

- **64-bit optimized opcodes**: Specialized instructions for faster 64-bit operations
- **Caching**: Module compilation caching for improved performance
- **Stack optimization**: Improved stack handling and memory management

## Use Cases

### **Primary Applications**

- **Zero-Knowledge Proofs**: Only runtime optimized for ZK circuit generation
- **Blockchain Smart Contracts**: Fast execution with deterministic behavior
- **zkVMs**: Efficient execution in zero-knowledge virtual machines
- **Blended-VM Runtimes**: Supporting EVM, SVM, and Wasm in unified runtime

### **Performance-Critical Scenarios**

- **Per-transaction execution**: Fast module loading for blockchain applications
- **Consensus systems**: Deterministic execution across network nodes
- **Proof generation**: Optimized for ZK-SNARK/STARK proof creation

## Getting Started

### **Installation**

```bash
# Clone the repository
git clone https://github.com/fluentlabs-xyz/rwasm.git
cd rwasm

# Build and run all tests
make
```

## Integration Examples

### **Fluent Runtime Integration**

rWasm is used in the [Fluent](https://github.com/fluentlabs-xyz) project to execute smart contracts in a unified runtime
that supports EVM, SVM, and Wasm logic in a ZK-friendly way.

rWasm is the **only WebAssembly runtime specifically designed for zero-knowledge applications**, combining:

- **ZK-optimized binary format** for efficient proof generation
- **Blockchain-native features** for smart contract execution
- **Performance advantages** where it matters most (module loading)
- **Full Wasm compatibility** for easy adoption

## Contributing

We welcome contributions to rWasm!

## License

rWasm is open-source software licensed under [LICENSE](LICENSE).

---

**rWasm**: The first and only ZK-friendly WebAssembly runtime, purpose-built for the future of blockchain and
zero-knowledge applications.