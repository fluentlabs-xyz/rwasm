fn wasmtime_rwasm_version(manifest: &str) -> Option<&str> {
    manifest
        .lines()
        .find(|line| line.contains("package = \"wasmtime-rwasm\""))
        .and_then(|line| line.split("version = \"").nth(1))
        .and_then(|rest| rest.split('"').next())
}

#[test]
fn fuzz_wasmtime_oracle_matches_root_wasmtime_version() {
    let root = include_str!("../Cargo.toml");
    let fuzz = include_str!("../fuzz/Cargo.toml");

    assert_eq!(
        wasmtime_rwasm_version(fuzz),
        wasmtime_rwasm_version(root),
        "fuzz oracle must use the same wasmtime-rwasm version as rwasm"
    );
}
