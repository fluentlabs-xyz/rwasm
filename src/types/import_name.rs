use alloc::boxed::Box;

/// The name or namespace of an imported item.
#[derive(Debug, Clone, PartialOrd, PartialEq, Ord, Eq, Hash)]
pub struct ImportName {
    /// The name of the [`Module`] that defines the imported item.
    ///
    /// [`Module`]: [`super::Module`]
    pub(crate) module: Box<str>,
    /// The name of the imported item within the [`Module`] namespace.
    ///
    /// [`Module`]: [`super::Module`]
    pub(crate) field: Box<str>,
}

impl core::fmt::Display for ImportName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let module_name = &*self.module;
        let field_name = &*self.field;
        write!(f, "{module_name}::{field_name}")
    }
}

impl ImportName {
    /// Creates a new [`Import`] item.
    pub fn new(module: &str, field: &str) -> Self {
        Self {
            module: module.into(),
            field: field.into(),
        }
    }

    /// Returns the name of the [`Module`] that defines the imported item.
    ///
    /// [`Module`]: [`super::Module`]
    pub fn module(&self) -> &str {
        &self.module
    }

    /// Returns the name of the imported item within the [`Module`] namespace.
    ///
    /// [`Module`]: [`super::Module`]
    pub fn name(&self) -> &str {
        &self.field
    }
}
