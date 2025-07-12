#[derive(Default, Clone)]
pub struct VMState {
    pub clk: u32,
    pub shard: u32,
    pub sp: u32,
}

pub const MAX_CYCLE_FOR_OP: u32 = 8; // TODO determine this for all instructions
pub const MAX_CYCLE: u32 = 1 << 20;

impl VMState {
    pub fn next_cycle(&mut self) {
        if self.clk + MAX_CYCLE_FOR_OP > MAX_CYCLE {
            self.clk = 0;
            self.next_shard();
        } else {
            self.clk += 1;
        }
    }

    pub fn next_shard(&mut self) {
        self.shard += 1;
    }
}
