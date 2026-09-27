use crate::application::cli::ActivityRow;

/// view-model.md: the render fn reads a view-model, not the object graph.
pub fn render_row(row: &ActivityRow) -> String {
    format!("{} {}m", row.name, row.minutes)
}
