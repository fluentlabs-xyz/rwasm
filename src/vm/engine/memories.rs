use crate::{
    vm::{
        config::{Config, MemoryAllocationStrategy, PoolingAllocatorConfig},
        reusable_pool::ReusablePool,
    },
    GlobalMemory, IGlobalMemory, Pages,
};

pub trait MemoryAllocatorTr {
    fn allocate_memory(&mut self, initial_pages: Pages) -> GlobalMemory;

    fn recycle_memory(&mut self, memory: GlobalMemory);
}

pub struct OnDemandMemoryAllocator;

impl MemoryAllocatorTr for OnDemandMemoryAllocator {
    fn allocate_memory(&mut self, initial_pages: Pages) -> GlobalMemory {
        GlobalMemory::new(initial_pages)
    }

    fn recycle_memory(&mut self, _memory: GlobalMemory) {
        // we don't recycle memory for on-demand memory allocator
    }
}

pub struct PoolingMemoryAllocator {
    reusable_pool: ReusablePool<GlobalMemory>,
}

impl PoolingMemoryAllocator {
    pub fn new(config: &PoolingAllocatorConfig, default_memory_pages: u32) -> Self {
        let mut reusable_pool = ReusablePool::new(config.max_size);
        for _ in 0..config.initial_size {
            reusable_pool.recycle(GlobalMemory::new(Pages::new(default_memory_pages).unwrap()))
        }
        Self { reusable_pool }
    }
}

impl MemoryAllocatorTr for PoolingMemoryAllocator {
    fn allocate_memory(&mut self, initial_pages: Pages) -> GlobalMemory {
        if let Some(mut result) = self.reusable_pool.try_reuse_item() {
            result.resize_for(initial_pages);
            return result;
        }
        GlobalMemory::new(initial_pages)
    }

    fn recycle_memory(&mut self, mut memory: GlobalMemory) {
        memory.reset();
        self.reusable_pool.recycle(memory)
    }
}

pub enum MemoryAllocator {
    OnDemandMemoryAllocator(OnDemandMemoryAllocator),
    PoolingMemoryAllocator(PoolingMemoryAllocator),
}

impl MemoryAllocator {
    pub fn new(config: &Config) -> Self {
        match &config.memory_allocation_strategy {
            MemoryAllocationStrategy::OnDemand => {
                MemoryAllocator::OnDemandMemoryAllocator(OnDemandMemoryAllocator {})
            }
            MemoryAllocationStrategy::Pooling(pooling_config) => {
                MemoryAllocator::PoolingMemoryAllocator(PoolingMemoryAllocator::new(
                    pooling_config,
                    config.default_memory_pages,
                ))
            }
        }
    }
}

impl MemoryAllocatorTr for MemoryAllocator {
    fn allocate_memory(&mut self, initial_pages: Pages) -> GlobalMemory {
        match self {
            MemoryAllocator::OnDemandMemoryAllocator(allocator) => {
                allocator.allocate_memory(initial_pages)
            }
            MemoryAllocator::PoolingMemoryAllocator(allocator) => {
                allocator.allocate_memory(initial_pages)
            }
        }
    }

    fn recycle_memory(&mut self, memory: GlobalMemory) {
        match self {
            MemoryAllocator::OnDemandMemoryAllocator(allocator) => allocator.recycle_memory(memory),
            MemoryAllocator::PoolingMemoryAllocator(allocator) => allocator.recycle_memory(memory),
        }
    }
}
