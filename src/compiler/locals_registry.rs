/// Counts the local variables of the function being translated.
///
/// # Note
///
/// In WebAssembly function parameters are also local variables.
///
/// The Wasm binary encoding declares up to `u32::MAX` locals in a few bytes, so the registry only
/// tracks their number; each local's type lives on the translator's operand-type stack
/// (`TypeStack`), which resolves the slot depth of a local access in O(1) without a per-usage
/// cache.
#[derive(Debug, Default)]
pub struct LocalsRegistry {
    /// The number of registered local variables.
    len_registered: u32,
}

impl LocalsRegistry {
    /// Returns the number of registered local variables.
    ///
    /// # Note
    ///
    /// Since in WebAssembly function parameters are also local variables,
    /// this function actually returns the number of function parameters
    /// and explicitly defined local variables.
    pub(crate) fn len_registered(&self) -> u32 {
        self.len_registered
    }

    /// Registers an `amount` of local variables.
    ///
    /// # Panics
    ///
    /// If too many local variables have been registered.
    pub fn register_locals(&mut self, amount: u32) {
        if amount == 0 {
            return;
        }
        self.len_registered = self.len_registered.checked_add(amount).unwrap_or_else(|| {
            panic!(
                "tried to register too many local variables for the function: got {}, additional {amount}",
                self.len_registered
            )
        });
    }
}
