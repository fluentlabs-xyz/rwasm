all: build_wasm build_debug

.PHONY: build_wasm
build_wasm:
	cd ../benchmarks && $(MAKE)

.PHONY: build_debug
build_debug:
	cargo build --profile dev

.PHONY: build_release
build_release:
	cargo build --profile release

.PHONY: perf_graph
perf_graph:
	perf.sh target/debug/perf1 main
	mv flame.svg flame_debug.svg
	#perf.sh target/release/perf1 main
	#mv flame.svg flame_release.svg

.PHONY: all_seq
all_seq:
	$(MAKE) build_wasm
	$(MAKE) build_debug
	$(MAKE) perf_graph

#.PHONY: flamegraph_perf1
#flamegraph_perf1:
#	echo -1 | sudo tee /proc/sys/kernel/perf_event_paranoid
#	cargo flamegraph --dev --bin perf1 --no-inline --root -o flamegraph_perf1.svg -c "record -e branch-misses,user_time,system_time,instructions -F MAX --call-graph lbr -g"
#	echo 4 | sudo tee /proc/sys/kernel/perf_event_paranoid
