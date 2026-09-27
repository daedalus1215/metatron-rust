//! One function that reads the disk and draws a widget.

use ratatui::widgets::Paragraph;
use std::fs;

pub fn render_file(path: &str) -> Paragraph<'static> {
    let body = fs::read_to_string(path).unwrap_or_default();
    Paragraph::new(body)
}

/// The control: rendering only.
pub fn render_blank() -> Paragraph<'static> {
    Paragraph::new(String::new())
}
