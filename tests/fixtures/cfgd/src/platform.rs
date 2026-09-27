// A second file, so "one diagnostic per file" is distinguishable from "one per
// crate": this file must produce its own line, counted separately.
#[cfg(windows)]
pub struct Win;

#[cfg(windows)]
pub struct AlsoWin;

#[cfg(unix)]
pub struct Unix;
