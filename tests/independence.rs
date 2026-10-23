//! The router depends on nothing: no runtime dependency of any kind, and
//! never Ogham, in any table.
//!
//! An empty `[dependencies]` is the guarantee that a consumer can put any
//! UI layer on top of this crate. Dev-dependencies are allowed — the
//! examples need a UI toolkit to drive — but not Ogham's side of the
//! stack, because an example that needed it would prove the opposite of
//! what the examples are for.

/// Tables that ship with the library, and so must stay empty.
const SHIPPED: &[&str] = &["[dependencies]", "[build-dependencies]"];

/// Packages no table may name: Ogham, and the crates that bind it to this
/// one.
const FORBIDDEN: &[&str] = &["ogham", "contract", "driver"];

#[test]
fn the_library_depends_on_nothing_and_never_on_ogham() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = std::fs::read_to_string(&path).expect("Cargo.toml");
    let mut table = String::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            table = line.to_string();
            continue;
        }
        if !table.contains("dependencies") || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let key = line.split('=').next().unwrap_or("").trim();
        assert!(
            !SHIPPED.contains(&table.as_str()),
            "{table} declares `{key}`: the router ships with no dependencies"
        );
        assert!(
            !FORBIDDEN.contains(&key),
            "{table} declares `{key}`: the router must not depend on Ogham"
        );
    }
}
