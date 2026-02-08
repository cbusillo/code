// Compile-time embedded version string.
// Prefer the CODE_VERSION provided by CI; fall back to the package
// version for local builds.
pub const CODE_VERSION: &str = {
    match option_env!("CODE_VERSION") {
        Some(v) => v,
        None => env!("CARGO_PKG_VERSION"),
    }
};

#[inline]
pub fn version() -> &'static str {
    CODE_VERSION
}
