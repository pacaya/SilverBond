use std::time::Duration;

pub(crate) const fn test_budget(d: Duration) -> Duration {
    d.saturating_mul(3)
}
