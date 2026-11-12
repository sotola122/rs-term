//! Decides when the screen needs a draw, so an idle session costs no CPU.

use std::time::{Duration, Instant};

use crate::status::StatusView;

/// Even with nothing visible changing, draw this often (covers anything the
/// other triggers cannot see, such as a terminal that lost its contents).
const MAX_IDLE: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct RedrawGate {
    last_view: Option<StatusView>,
    last_draw: Option<Instant>,
}

impl RedrawGate {
    /// `dirty` is true when an input event, a resize, or received data was
    /// handled since the last draw. The status view catches state changes that
    /// need no event (link, log, messages, counters).
    pub fn should_draw(&mut self, dirty: bool, view: StatusView, now: Instant) -> bool {
        let changed = self.last_view.as_ref() != Some(&view);
        let stale = self
            .last_draw
            .is_none_or(|at| now.duration_since(at) >= MAX_IDLE);
        if dirty || changed || stale {
            self.last_view = Some(view);
            self.last_draw = Some(now);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::LinkState;

    fn view() -> StatusView {
        StatusView {
            colors: rs_term_core::default_palette().ui,
            prefix: false,
            prefix_label: "Ctrl+A".into(),
            port: "COM3".into(),
            frame: "115200 8N1".into(),
            encoding: "UTF-8".into(),
            link: LinkState::Open,
            log: "LOG off".into(),
            rx: 0,
            tx: 0,
            message: None,
        }
    }

    #[test]
    fn a_theme_change_alone_draws() {
        let mut gate = RedrawGate::default();
        let start = Instant::now();
        assert!(gate.should_draw(false, view(), start));
        let mut themed = view();
        themed.colors = rs_term_core::palette_by_name("light").unwrap().ui;
        assert!(gate.should_draw(false, themed, start + Duration::from_millis(10)));
    }

    #[test]
    fn idle_does_not_draw_until_something_changes() {
        let mut gate = RedrawGate::default();
        let start = Instant::now();
        assert!(gate.should_draw(false, view(), start), "first frame");
        let soon = start + Duration::from_millis(100);
        assert!(!gate.should_draw(false, view(), soon));
        assert!(gate.should_draw(true, view(), soon), "an event was handled");
        assert!(!gate.should_draw(false, view(), soon));
        let mut changed = view();
        changed.prefix = true;
        assert!(
            gate.should_draw(false, changed.clone(), soon),
            "status changed"
        );
        assert!(!gate.should_draw(false, changed, soon));
    }

    #[test]
    fn an_idle_second_forces_a_draw() {
        let mut gate = RedrawGate::default();
        let start = Instant::now();
        assert!(gate.should_draw(false, view(), start));
        assert!(!gate.should_draw(false, view(), start + Duration::from_millis(999)));
        assert!(gate.should_draw(false, view(), start + Duration::from_millis(1000)));
    }
}
