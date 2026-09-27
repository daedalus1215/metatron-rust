pub trait NoteStore {
    fn read_note(&self, path: &str) -> String;
}
