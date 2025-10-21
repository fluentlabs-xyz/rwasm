use crate::{N_DEFAULT_STACK_SIZE, N_MAX_STACK_SIZE};

#[derive(Debug, Clone)]
pub struct Config {
    pub memory_allocation_strategy: MemoryAllocationStrategy,
    pub reusable_stack: ReusableStackConfig,
    pub default_memory_pages: u32,
}

#[derive(Debug, Clone)]
pub struct ReusableStackConfig {
    pub initial_len: usize,
    pub maximum_len: usize,
}

impl Default for ReusableStackConfig {
    fn default() -> Self {
        Self {
            initial_len: N_DEFAULT_STACK_SIZE,
            maximum_len: N_MAX_STACK_SIZE,
        }
    }
}

/// A default memory pages (1 page equal to 64kB)
const DEFAULT_MEMORY_PAGES: u32 = 1;

impl Default for Config {
    fn default() -> Self {
        Self {
            memory_allocation_strategy: MemoryAllocationStrategy::OnDemand,
            reusable_stack: ReusableStackConfig::default(),
            default_memory_pages: DEFAULT_MEMORY_PAGES,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub enum MemoryAllocationStrategy {
    #[default]
    OnDemand,
    Pooling(PoolingAllocatorConfig),
}

#[derive(Debug, Clone)]
pub struct PoolingAllocatorConfig {
    pub initial_size: usize,
    pub max_size: usize,
}

const DEFAULT_REUSABLE_POOL_KEEP: usize = 128;
const DEFAULT_REUSABLE_POOL_MAX: usize = 1024;

impl Default for PoolingAllocatorConfig {
    fn default() -> Self {
        PoolingAllocatorConfig {
            initial_size: DEFAULT_REUSABLE_POOL_KEEP,
            max_size: DEFAULT_REUSABLE_POOL_MAX,
        }
    }
}
