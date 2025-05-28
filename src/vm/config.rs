#[derive(Default)]
pub struct ExecutorConfig {
    pub fuel_enabled: bool,
    pub fuel_limit: Option<u64>,
    #[cfg(feature = "tracing")]
    pub trace_enabled: bool,
    pub default_pc: Option<usize>,
}

impl ExecutorConfig {
    pub fn new() -> Self {
        Self {
            fuel_enabled: true,
            fuel_limit: None,
            #[cfg(feature = "tracing")]
            trace_enabled: false,
            default_pc: None,
        }
    }

    pub fn fuel_enabled(mut self, fuel_enabled: bool) -> Self {
        self.fuel_enabled = fuel_enabled;
        self
    }

    pub fn fuel_limit(mut self, fuel_limit: u64) -> Self {
        self.fuel_limit = Some(fuel_limit);
        self
    }

    #[cfg(feature = "tracing")]
    pub fn trace_enabled(mut self, trace_enabled: bool) -> Self {
        self.trace_enabled = trace_enabled;
        self
    }
}
