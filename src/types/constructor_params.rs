use alloc::vec::Vec;
use wasmparser::CustomSectionReader;

/// A constant string that represents the name of a custom section.
///
/// This constant is used to denote the name of a custom section named "input".
/// It is defined as a static string slice with a fixed value.
const CONSTRUCTOR_CUSTOM_SECTION_NAME: &str = "input";

#[derive(Default, Debug)]
#[repr(transparent)]
/// Optional binary blob passed to the module constructor via a custom section.
/// When present, it is extracted from the "input" custom section and delivered to the host.
pub struct ConstructorParams(Option<Vec<u8>>);

impl core::ops::Deref for ConstructorParams {
    type Target = Option<Vec<u8>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Into<Vec<u8>> for ConstructorParams {
    fn into(self) -> Vec<u8> {
        self.0.unwrap_or_default()
    }
}

impl ConstructorParams {
    pub fn try_parse(&mut self, reader: CustomSectionReader) {
        if reader.name() == CONSTRUCTOR_CUSTOM_SECTION_NAME {
            self.0 = Some(reader.data().to_vec());
        }
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0.unwrap_or_default()
    }
}
