// A crate whose configuration names the sequences that would otherwise close
// the page's own <script> element. Nothing here is exotic; the point is that
// every string in a view payload came from a file on disk.
pub struct Widget;

impl Widget {
    pub fn render(&self) -> String {
        String::from("widget")
    }
}
