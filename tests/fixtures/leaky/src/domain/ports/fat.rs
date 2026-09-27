/// port.md, interface segregation: 13 methods is more than one fake's
/// worth of work.
pub trait FatPort {
    fn op1(&self) -> i64;
    fn op2(&self) -> i64;
    fn op3(&self) -> i64;
    fn op4(&self) -> i64;
    fn op5(&self) -> i64;
    fn op6(&self) -> i64;
    fn op7(&self) -> i64;
    fn op8(&self) -> i64;
    fn op9(&self) -> i64;
    fn op10(&self) -> i64;
    fn op11(&self) -> i64;
    fn op12(&self) -> i64;
    fn op13(&self) -> i64;
}
