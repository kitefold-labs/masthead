//! Hover intent for the popover, as a state machine with no UI types.
//!
//! The pointer must rest on an item for [`OPEN_DELAY`] before its popover
//! opens, and the popover closes [`CLOSE_DELAY`] after the pointer leaves, so
//! sweeping across the bar does not flash popovers. Moving from one item to
//! another while a popover is open switches immediately. The daemon's
//! `hover:<metric>` tier is held exactly while a popover is open.

use std::time::Duration;

pub const OPEN_DELAY: Duration = Duration::from_millis(300);
pub const CLOSE_DELAY: Duration = Duration::from_millis(150);

/// What the UI must do in response to an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Deliver `open_timer(seq)` after [`OPEN_DELAY`].
    OpenTimer(u64),
    /// Deliver `close_timer(seq)` after [`CLOSE_DELAY`].
    CloseTimer(u64),
    /// Create the popover for a metric.
    Open(String),
    /// Destroy the popover.
    Close,
    /// Call `Subscribe("hover:<metric>")`.
    Subscribe(String),
    /// Call `Unsubscribe(handle)`.
    Unsubscribe(u32),
}

#[derive(Debug, Default)]
pub struct Hover {
    pointer: Option<String>,
    open: Option<String>,
    handle: Option<u32>,
    seq: u64,
}

impl Hover {
    /// The metric whose popover is open.
    pub fn open(&self) -> Option<&str> {
        self.open.as_deref()
    }

    pub fn enter(&mut self, id: &str) -> Vec<Effect> {
        self.pointer = Some(id.to_owned());
        self.seq += 1;
        match self.open.as_deref() {
            // Back on the open item: the pending close is cancelled by the new seq.
            Some(open) if open == id => Vec::new(),
            Some(_) => {
                let mut e = self.close();
                e.extend(self.show(id));
                e
            }
            None => vec![Effect::OpenTimer(self.seq)],
        }
    }

    pub fn exit(&mut self, id: &str) -> Vec<Effect> {
        if self.pointer.as_deref() == Some(id) {
            self.pointer = None;
        }
        self.seq += 1;
        if self.open.is_some() { vec![Effect::CloseTimer(self.seq)] } else { Vec::new() }
    }

    /// The pointer left the applet surface entirely. Widgets do not always see
    /// an exit when that happens, so the surface reports it separately.
    pub fn left_surface(&mut self) -> Vec<Effect> {
        self.pointer = None;
        self.seq += 1;
        if self.open.is_some() { vec![Effect::CloseTimer(self.seq)] } else { Vec::new() }
    }

    pub fn open_timer(&mut self, seq: u64) -> Vec<Effect> {
        if seq != self.seq || self.open.is_some() {
            return Vec::new();
        }
        match self.pointer.clone() {
            Some(id) => self.show(&id),
            None => Vec::new(),
        }
    }

    pub fn close_timer(&mut self, seq: u64) -> Vec<Effect> {
        if seq != self.seq || self.pointer.is_some() {
            return Vec::new();
        }
        self.close()
    }

    /// A click opens the window; the popover gets out of the way and stays
    /// closed until the pointer enters an item again.
    pub fn click(&mut self) -> Vec<Effect> {
        self.seq += 1;
        self.close()
    }

    /// The compositor closed the popover on its own.
    pub fn closed_externally(&mut self) -> Vec<Effect> {
        self.seq += 1;
        self.open = None;
        self.handle.take().map(Effect::Unsubscribe).into_iter().collect()
    }

    /// A `Subscribe` reply arrived. A reply for a popover that already closed
    /// is released straight away.
    pub fn subscribed(&mut self, id: &str, handle: u32) -> Vec<Effect> {
        if self.open.as_deref() == Some(id) && self.handle.is_none() {
            self.handle = Some(handle);
            Vec::new()
        } else {
            vec![Effect::Unsubscribe(handle)]
        }
    }

    /// The daemon restarted, so its subscriptions are gone.
    pub fn daemon_restarted(&mut self) -> Vec<Effect> {
        self.handle = None;
        self.open.clone().map(Effect::Subscribe).into_iter().collect()
    }

    fn show(&mut self, id: &str) -> Vec<Effect> {
        self.open = Some(id.to_owned());
        vec![Effect::Open(id.to_owned()), Effect::Subscribe(id.to_owned())]
    }

    fn close(&mut self) -> Vec<Effect> {
        if self.open.take().is_none() {
            return Vec::new();
        }
        let mut e = vec![Effect::Close];
        e.extend(self.handle.take().map(Effect::Unsubscribe));
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Effect::{Close, CloseTimer, Open, OpenTimer, Subscribe, Unsubscribe};

    fn sub(id: &str) -> Effect {
        Subscribe(id.into())
    }

    #[test]
    fn rest_then_leave() {
        let mut h = Hover::default();
        assert_eq!(h.enter("cpu"), vec![OpenTimer(1)]);
        assert_eq!(h.open_timer(1), vec![Open("cpu".into()), sub("cpu")]);
        assert_eq!(h.open(), Some("cpu"));
        assert_eq!(h.subscribed("cpu", 7).len(), 0);
        assert_eq!(h.exit("cpu"), vec![CloseTimer(2)]);
        assert_eq!(h.close_timer(2), vec![Close, Unsubscribe(7)]);
        assert_eq!(h.open(), None);
    }

    #[test]
    fn leaving_the_surface_closes() {
        let mut h = Hover::default();
        h.enter("cpu");
        assert!(h.left_surface().is_empty(), "nothing open yet");
        assert!(h.open_timer(1).is_empty(), "the pending open is cancelled");
        h.enter("cpu");
        h.open_timer(3);
        assert_eq!(h.left_surface(), vec![CloseTimer(4)]);
        assert_eq!(h.close_timer(4), vec![Close]);
    }

    #[test]
    fn sweeping_does_not_open() {
        let mut h = Hover::default();
        h.enter("cpu");
        h.exit("cpu");
        assert!(h.open_timer(1).is_empty(), "stale timer");
        assert!(h.open_timer(2).is_empty(), "pointer gone");
        assert_eq!(h.enter("mem"), vec![OpenTimer(3)]);
        assert_eq!(h.open_timer(3), vec![Open("mem".into()), sub("mem")]);
    }

    #[test]
    fn switching_and_returning() {
        let mut h = Hover::default();
        h.enter("cpu");
        h.open_timer(1);
        h.subscribed("cpu", 1);
        assert_eq!(h.exit("cpu"), vec![CloseTimer(2)]);
        assert_eq!(h.enter("mem"), vec![Close, Unsubscribe(1), Open("mem".into()), sub("mem")]);
        assert!(h.close_timer(2).is_empty(), "the close for cpu is stale");
        assert_eq!(h.exit("mem"), vec![CloseTimer(4)]);
        assert!(h.enter("mem").is_empty(), "back on the open item keeps it");
        assert_eq!(h.close_timer(4).len(), 0);
        assert_eq!(h.open(), Some("mem"));
    }

    #[test]
    fn late_replies_and_restarts() {
        let mut h = Hover::default();
        assert_eq!(h.subscribed("cpu", 9), vec![Unsubscribe(9)]);
        h.enter("cpu");
        h.open_timer(1);
        assert_eq!(h.click(), vec![Close]);
        assert_eq!(h.subscribed("cpu", 3), vec![Unsubscribe(3)], "reply after close");
        assert_eq!(h.open_timer(1).len(), 0);
        h.enter("gpu");
        h.open_timer(h.seq);
        h.subscribed("gpu", 4);
        assert_eq!(h.daemon_restarted(), vec![sub("gpu")]);
        h.subscribed("gpu", 5);
        assert_eq!(h.subscribed("gpu", 6), vec![Unsubscribe(6)], "only one handle held");
        assert_eq!(h.closed_externally(), vec![Unsubscribe(5)]);
        assert_eq!(h.click().len(), 0);
        assert_eq!(Hover::default().daemon_restarted().len(), 0);
    }
}
