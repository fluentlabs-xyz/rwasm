#![allow(unused)]

mod context;
mod descriptor;
mod error;
mod handler;
mod profile;
mod run;

use self::{
    context::TestContext,
    descriptor::{TestDescriptor, TestSpan},
    error::TestError,
    profile::TestProfile,
};
use ::rwasm_legacy::{engine::RwasmConfig, Config};

macro_rules! define_tests {
    (
        let folder = $test_folder:literal;
        let runner = $runner_fn:path;

        $( $(#[$attr:meta])* fn $test_name:ident($file_name:expr); )*
    ) => {
        $(
            #[test]
            $( #[$attr] )*
            fn $test_name() {
                $runner_fn(&format!("{}/{}", $test_folder, $file_name))
            }
        )*
    };
}

macro_rules! define_spec_tests {
    (
        let runner = $runner_fn:path;

        $( $(#[$attr:meta])* fn $test_name:ident($file_name:expr); )*
    ) => {
        define_tests! {
            let folder = "testsuite";
            let runner = $runner_fn;

            $(
                $( #[$attr] )*
                fn $test_name($file_name);
            )*
        }
    };
}

pub(crate) const ENABLE_32_BIT_TRANSLATOR: bool = false;

define_spec_tests! {
    let runner = run::run_wasm_spec_test;

    fn wasm_address("address");
    fn wasm_align("align");
    fn wasm_binary_leb128("binary-leb128");
    fn wasm_binary("binary");
    fn wasm_block("block");
    fn wasm_br("br");
    fn wasm_br_if("br_if");
    fn wasm_br_table("br_table");
    fn wasm_bulk("bulk");
    fn wasm_call("call");
    fn wasm_call_indirect("call_indirect");
    // fn wasm_extended_const_data("proposals/extended-const/data"); // ImportedMemoriesAreDisabled
    // fn wasm_extended_const_elem("proposals/extended-const/elem"); // ImportedMemoriesAreDisabled
    // fn wasm_extended_const_global("proposals/extended-const/global"); // ImportedMemoriesAreDisabled
    fn wasm_return_call("proposals/tail-call/return_call");
    fn wasm_return_call_indirect("proposals/tail-call/return_call_indirect");
    fn wasm_comments("comments");
    fn wasm_const("const");
    fn wasm_conversions("conversions");
    fn wasm_custom("custom");
    fn wasm_data("data");
    fn wasm_elem("elem");
    fn wasm_endianness("endianness");
    fn wasm_exports("exports");
    fn wasm_f32("f32");
    fn wasm_f32_bitwise("f32_bitwise");
    fn wasm_f32_cmp("f32_cmp");
    fn wasm_f64("f64");
    fn wasm_f64_bitwise("f64_bitwise");
    fn wasm_f64_cmp("f64_cmp");
    fn wasm_fac("fac"); //wrong branch
    fn wasm_float_exprs("float_exprs");
    fn wasm_float_literals("float_literals");
    fn wasm_float_memory("float_memory");
    fn wasm_float_misc("float_misc");
    fn wasm_forward("forward");
    fn wasm_func("func");
    fn wasm_func_ptrs("func_ptrs");
    fn wasm_global("global");
    fn wasm_i32("i32");
    fn wasm_i64("i64");
    fn wasm_if("if");
    // fn wasm_imports("imports"); // UnknownImport
    fn wasm_inline_module("inline-module");
    fn wasm_int_exprs("int_exprs");
    fn wasm_int_literals("int_literals");
    fn wasm_labels("labels");
    fn wasm_left_to_right("left-to-right");
    // fn wasm_linking("linking"); // UnknownImport
    fn wasm_load("load");
    fn wasm_local_get("local_get");
    fn wasm_local_set("local_set");
    fn wasm_local_tee("local_tee");
    fn wasm_loop("loop");
    fn wasm_memory("memory");
    fn wasm_memory_copy("memory_copy");
    fn wasm_memory_fill("memory_fill");
    fn wasm_memory_grow("memory_grow");
    fn wasm_memory_init("memory_init");
    fn wasm_memory_redundancy("memory_redundancy");
    fn wasm_memory_size("memory_size");
    fn wasm_memory_trap("memory_trap");
    fn wasm_names("names");
    fn wasm_nop("nop");
    // fn wasm_ref_func("ref_func"); // UnknownImport
    fn wasm_ref_is_null("ref_is_null");
    fn wasm_ref_null("ref_null");
    fn wasm_return("return");
    fn wasm_select("select");
    fn wasm_skip_stack_guard_page("skip-stack-guard-page");
    fn wasm_stack("stack");
    fn wasm_start("start");
    fn wasm_store("store");
    fn wasm_switch("switch");
    fn wasm_table_sub("table-sub");
    fn wasm_table("table");
    // fn wasm_table_copy("table_copy"); // UnknownImport
    fn wasm_table_fill("table_fill");
    fn wasm_table_get("table_get");
    fn wasm_table_grow("table_grow");
    // fn wasm_table_init("table_init"); // UnknownImport
    fn wasm_table_set("table_set");
    fn wasm_table_size("table_size");
    fn wasm_token("token");
    fn wasm_traps("traps");
    fn wasm_type("type");
    fn wasm_unreachable("unreachable");
    fn wasm_unreached_invalid("unreached-invalid");
    fn wasm_unreached_valid("unreached-valid");
    fn wasm_unwind("unwind");
    fn wasm_utf8_custom_section_id("utf8-custom-section-id");
    fn wasm_utf8_import_field("utf8-import-field");
    fn wasm_utf8_import_module("utf8-import-module");
    fn wasm_utf8_invalid_encoding("utf8-invalid-encoding");
}
