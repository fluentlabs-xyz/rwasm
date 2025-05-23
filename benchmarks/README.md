These benchmarks are not finalized yet.
Here we test the cost of running SP1's RISC-V, Wasmi & rWasm in Ethereum execution environment.
When it's required to load the account from storage, only raw bytes.

Current results:

| Test          | Native | Risc-V | Wasmi | rWasm |
|---------------|--------|--------|-------|-------|
| Fibonacci(43) | ~12ns  | ~7ms   | ~6us  | ~2us  |

PS:

1. Risc-V is slow because of tracing
2. Wasmi is slow because of Wasm validation rules

Tested on Apple M3 MAX