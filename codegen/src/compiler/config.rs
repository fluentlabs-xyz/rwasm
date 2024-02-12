use crate::InstructionSet;

#[derive(Debug, Clone)]
pub struct CompilerConfig {
    pub(crate) fuel_consume: bool,
    pub(crate) tail_call: bool,
    pub(crate) extended_const: bool,
    pub(crate) translate_sections: bool,
    pub(crate) with_state: bool,
    pub(crate) translate_func_as_inline: bool,
    pub(crate) type_check: bool,
    pub(crate) input_code: Option<InstructionSet>,
    pub(crate) output_code: Option<InstructionSet>,
    pub(crate) global_start_index: Option<u32>,
    pub(crate) swap_stack_params: bool,
    pub(crate) with_router: bool,
    pub(crate) with_magic_prefix: bool,
}

impl Default for CompilerConfig {
    fn default() -> Self {
        Self {
            fuel_consume: true,
            tail_call: true,
            extended_const: true,
            translate_sections: true,
            with_state: false,
            translate_func_as_inline: false,
            type_check: true,
            input_code: None,
            output_code: None,
            global_start_index: None,
            swap_stack_params: true,
            with_router: true,
            with_magic_prefix: true,
        }
    }
}

impl CompilerConfig {
    pub fn fuel_consume(mut self, value: bool) -> Self {
        self.fuel_consume = value;
        self
    }

    pub fn type_check(mut self, value: bool) -> Self {
        self.type_check = value;
        self
    }

    pub fn tail_call(mut self, value: bool) -> Self {
        self.tail_call = value;
        self
    }

    pub fn extended_const(mut self, value: bool) -> Self {
        self.extended_const = value;
        self
    }

    pub fn translate_sections(mut self, value: bool) -> Self {
        self.translate_sections = value;
        self
    }

    pub fn with_state(mut self, value: bool) -> Self {
        self.with_state = value;
        self
    }

    pub fn with_router(mut self, value: bool) -> Self {
        self.with_router = value;
        self
    }

    pub fn with_magic_prefix(mut self, value: bool) -> Self {
        self.with_magic_prefix = value;
        self
    }

    pub fn translate_func_as_inline(mut self, value: bool) -> Self {
        self.translate_func_as_inline = value;
        self
    }

    pub fn with_input_code(mut self, input_code: InstructionSet) -> Self {
        self.input_code = Some(input_code);
        self
    }

    pub fn with_output_code(mut self, output_code: InstructionSet) -> Self {
        self.output_code = Some(output_code);
        self
    }

    pub fn with_global_start_index(mut self, global_start_index: u32) -> Self {
        self.global_start_index = Some(global_start_index);
        self
    }

    pub fn with_swap_stack_params(mut self, swap_stack_params: bool) -> Self {
        self.swap_stack_params = swap_stack_params;
        self
    }
}
