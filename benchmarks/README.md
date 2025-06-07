In these benchmarks, we emulate the Ethereum execution environment, where a smart contract must be loaded from its
binary representation before execution.

The binary can be loaded either from a cache or directly from the state trie.
If we load the binary from the state, we need to parse, verify, and execute it (optionally caching it afterward).
For cached binaries, only execution is required.

**Current results:**

| Test    | Native | rWasm | rWasm (no-cache) | Wasmi | Wasmi (no-cache) |
|---------|--------|-------|------------------|-------|------------------|
| fib(47) | 12ns   | 484ns | 879ns            | 341ns | 5379ns           |

Tested on Apple M3 MAX.

**PS:** We also benchmarked SP1's RISC-V implementation but removed it from the table since it was too slow (\~7 ms for
fib).

---

rWasm delivers performance very close to Wasmi (the minor difference is due to the entrypoint logic that rWasm
requires).
This happens because rWasm has a dedicated function to initialize all necessary data, and this data is reset before each
benchmark iteration.
Wasmi caches initialization as well, which is why it sometimes appears faster.

In practice, the most common scenario is execution without cache ("no-cache").
In this case, rWasm shows significant performance improvements over Wasmi (up to 10x faster).
There are techniques available to further reduce binary decoding costs, but they haven’t been applied yet.
With optimized binary decoding, execution time could be as low as \~400–500 ns.