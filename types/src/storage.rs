use crate::Account;
use alloy_primitives::{Address, Bytes, U256};

pub trait AccountDb {
    fn get_account(&mut self, address: &Address) -> Option<Account>;

    fn update_account(&mut self, address: &Address, account: &Account);

    fn get_storage(&mut self, address: &Address, index: &U256) -> Option<U256>;

    fn update_storage(&mut self, address: &Address, index: &U256, value: &U256);

    fn get_node(&mut self, key: &[u8]) -> Option<Bytes>;

    fn update_node(&mut self, key: &[u8], value: Bytes);

    fn get_preimage(&mut self, key: &[u8]) -> Option<Bytes>;

    fn update_preimage(&mut self, key: &[u8], value: Bytes);
}
