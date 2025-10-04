all: all_sequentially

.PHONY: build_wasm
build_wasm:
	cd ../benchmarks && $(MAKE)

.PHONY: build
build:
	cargo build --profile dev

.PHONY: fibonacci_perf32_flame
fibonacci_perf32_flame:
	perf.sh target/debug/fibonacci_perf32 main
	mv flame.svg fibonacci_perf32_flame.svg

.PHONY: fibonacci_perf64_flame
fibonacci_perf64_flame:
	perf.sh target/debug/fibonacci_perf64 main
	mv flame.svg fibonacci_perf64_flame.svg

.PHONY: evm_fibonacci_perf_flame
evm_fibonacci_perf_flame:
	perf.sh target/debug/evm_fibonacci_perf
	mv flame.svg evm_fibonacci_perf_flame.svg

.PHONY: load_perf_flame
load_perf_flame:
	perf.sh target/debug/load_perf main
	mv flame.svg load_perf_flame.svg

.PHONY: all_sequentially
all_sequentially:
	$(MAKE) build_wasm
	$(MAKE) build
	#$(MAKE) evm_fibonacci_perf_flame
	#$(MAKE) fibonacci_perf32_flame
	$(MAKE) fibonacci_perf64_flame
	#$(MAKE) load_perf_flame

.PHONY: clean
clean:
	cargo clean
	rm -rf out.*
	rm -rf perf.data*
