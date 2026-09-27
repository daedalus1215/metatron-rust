//! One module, one layer. The control: a detector that calls this mixed
//! is reporting on the config's coverage, not on the code's structure.

pub enum Outcome {
    Ok,
    Failed,
}

pub fn is_failed(o: &Outcome) -> bool {
    matches!(o, Outcome::Failed)
}
