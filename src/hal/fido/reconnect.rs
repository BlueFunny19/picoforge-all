//! Reconnection state shared by the UI's reset worker and its regression tests.
use std::time::Duration;
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) enum Reconnection {
    #[default]
    Connected,
    Unplugged,
    Reconnected,
}
impl Reconnection {
    pub(crate) fn observe(
        &mut self,
        target_present: bool,
        elapsed: Duration,
    ) -> Result<bool, &'static str> {
        if elapsed >= Duration::from_secs(30) {
            return Err("Timed out waiting for this device to be unplugged and reconnected.");
        }
        match (&*self, target_present) {
            (Self::Connected, false) => *self = Self::Unplugged,
            (Self::Unplugged, true) => *self = Self::Reconnected,
            _ => {}
        }
        Ok(*self == Self::Reconnected)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reset_requires_actual_disconnect_then_same_target() {
        let mut state = Reconnection::default();
        assert_eq!(state.observe(true, Duration::ZERO), Ok(false));
        assert_eq!(state.observe(true, Duration::from_secs(5)), Ok(false));
        assert_eq!(state.observe(false, Duration::from_secs(6)), Ok(false));
        // A different key present is still target_present=false.
        assert_eq!(state.observe(false, Duration::from_secs(8)), Ok(false));
        assert_eq!(state.observe(true, Duration::from_secs(9)), Ok(true));
    }
    #[test]
    fn timeout_is_bounded_in_both_phases() {
        for disconnected in [false, true] {
            let mut state = Reconnection::default();
            state.observe(!disconnected, Duration::ZERO).unwrap();
            assert!(state.observe(true, Duration::from_secs(30)).is_err());
        }
    }
}
