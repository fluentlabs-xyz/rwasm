# rWASM Documentation

This folder is the canonical technical documentation for `fluentlabs-xyz/rwasm`.

## Start here

1. [Architecture](./architecture.md)
2. [Compilation & Execution Pipeline](./pipeline.md)
3. [Module Format](./module-format.md)
4. [VM, Fuel, and Tracing](./vm-and-fuel.md)
5. [Opcode Specification](./opcodes.md)
6. [Contributor Guide](./contributor-guide.md)

## Audience split

- **Protocol / VM engineers**: architecture, module format, opcode spec
- **Runtime integrators**: pipeline, VM/fuel, host syscall model
- **Contributors**: root `README.md`, `Makefile`, CI workflows

## Source-of-truth policy

- Behavior is implemented in `src/**`.
- If docs conflict with code, code wins.
- Update docs in the same PR as semantic changes to compiler/VM/opcodes.
