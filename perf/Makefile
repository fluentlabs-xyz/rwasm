all: build_wasm build_debug

.PHONY: build_wasm
build_wasm:
	cd ../benchmarks && $(MAKE)

.PHONY: build_debug
build_debug:
	cargo build --profile dev

.PHONY: fibonacci_perf_flame
fibonacci_perf_flame:
	perf.sh target/debug/fibonacci_perf main
	mv flame.svg fibonacci_perf_flame.svg

.PHONY: load_perf_flame
load_perf_flame:
	perf.sh target/debug/load_perf main
	mv flame.svg load_perf_flame.svg

.PHONY: all_seq
all_seq:
	$(MAKE) build_wasm
	$(MAKE) build_debug
	$(MAKE) fibonacci_perf_flame
	#$(MAKE) load_perf_flame

.PHONY: flamegraph_fibonacci_perf
flamegraph_fibonacci_perf:
	echo -1 | sudo tee /proc/sys/kernel/perf_event_paranoid
	cargo flamegraph --dev --bin fibonacci_perf --root -o fibonacci_perf_flame1.svg -c "record -e branch-misses -e user_time -e system_time -e instructions -z10 -r0 -F MAX --call-graph lbr -g"
	echo 4 | sudo tee /proc/sys/kernel/perf_event_paranoid
