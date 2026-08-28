# Benchmarks

These benchmarks model the two ways an execution environment reaches a contract, following the
Ethereum pattern: either the module is already instantiated and cached, or it has to be loaded and
instantiated from the state trie on every call.

The two cases have very different costs, so each backend is measured both ways:

- **warm** — instantiation is hoisted out of the measured loop; only execution is timed.
- **cold** — a fresh store and instance per iteration, then execution.

Both backends meter fuel, so neither is credited for skipping work the other does.

## Running

```bash
cargo bench --bench bench
```

## Current results

`fib(43)`, Apple M5 Max:

| Backend            | warm    | cold    |
|--------------------|---------|---------|
| Native Rust        | 10.5 ns | —       |
| rWasm interpreter  | 322 ns  | 476 ns  |
| Wasmtime (JIT)     | 109 ns  | 5.48 µs |

Wasmtime executes compiled code, so it wins the warm row by roughly 3x; rWasm is an interpreter and
that gap is expected. The order reverses on the cold row, where Wasmtime's store and instance
construction costs about 5 µs, an order of magnitude more than an entire rWasm call.

## What these numbers do not tell you

`fib` is a 43-iteration integer loop. It measures interpreter dispatch and instantiation overhead,
not the memory traffic, host calls, or control flow of a real contract.

The cold row in particular is a floor rather than a typical figure. `examples/fib` is linked with a
zero-sized shadow stack (`-zstack-size=0`), so it declares a zero-page linear memory and
instantiation allocates nothing. A real module declares real memory, and cold instantiation then
pays to allocate and zero it — about 4 µs for the 1 MiB shadow stack `wasm-ld` reserves by default,
which on its own dwarfs everything in the table above.

## Why the warm row needs care

A warm executor is only meaningful because `fib` is pure integer arithmetic that leaves no guest
state behind. Reusing an executor is not a general-purpose mode, and the benchmark should not be
read as a claim that it is.

`StrategyExecutor::execute` performs no reset. The interpreter's own value and call stacks are
rebuilt on every call, so those are never the problem — but everything on the guest side survives:
mutable globals, linear memory, dropped data and element segment flags, and consumed fuel.

The sharp edge is a trap. A guest that traps mid-call never restores its shadow stack pointer, and
because nothing resets that global, every later call starts from the corrupted value and never
recovers. Measured on a module that decrements a shadow stack pointer and traps before restoring
it:

```
clean warm runs        sp = 65536, 65536, 65536
a run that traps       Err(UnreachableCodeReached)
warm runs after that   sp = 65520, 65520, 65520   <- permanently shifted
```

`RwasmStore::reset(false)` followed by re-running the entrypoint does restore globals and re-apply
data segments, which brings that example back to 65536. It does not zero linear memory: bytes
written outside a data segment survive the reset. There is no cheaper path back to a genuinely
clean instance than building one, which is exactly what the cold row measures.

So the warm row is a fair microbenchmark of execution throughput on a self-contained, non-trapping
kernel, and nothing more. Anything that traps, mutates memory, or depends on fresh globals has to
use the cold path.
