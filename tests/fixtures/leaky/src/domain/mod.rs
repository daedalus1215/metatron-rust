pub mod converters;
pub mod ports;
pub mod services;
pub mod use_cases;

pub struct Activity { pub id: i64, pub name: String }

#[derive(Clone, Copy, PartialEq)]
pub enum StartOutcome { Started, StartedNew, AlreadyRunning }

/// naming.md: the domain must not use "Dto".
#[derive(Clone, PartialEq)]
pub struct ActivityDto { pub name: String }
