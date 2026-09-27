use crate::domain::Activity;

/// converter.md: a converter is pure. This one does I/O and returns Result.
pub fn to_label(a: &Activity) -> Result<String, String> {
    let _ = std::fs::metadata("/tmp");
    Ok(a.name.clone())
}
