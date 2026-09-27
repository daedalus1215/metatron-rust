use crate::domain::Activity;

/// converter.md: pure mapping. No port, no Result, no I/O.
pub fn to_display_name(a: &Activity) -> String {
    a.name.to_uppercase()
}
