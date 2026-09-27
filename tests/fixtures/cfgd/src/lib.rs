// `#[cfg]` is recorded, not evaluated: these items are in the model whether or
// not this build compiles them. The model has to admit that.
#[cfg(feature = "fast")]
pub struct Fast;

#[cfg(feature = "fast")]
pub mod fast {
    pub fn go() -> u8 {
        1
    }
}

#[cfg(target_os = "linux")]
pub fn on_linux() -> bool {
    true
}

pub struct Always;

pub mod platform;
