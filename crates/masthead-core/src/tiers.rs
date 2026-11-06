//! Who is looking, and therefore what the daemon samples.
//!
//! Each face calls `Subscribe(tier)` and gets a handle. A subscription belongs
//! to the D-Bus connection that made it and ends when that connection goes
//! away, so a crashed face can never leave expensive sampling running.

use crate::MetricId;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Bar values: global counters at 1 Hz.
    Panel,
    /// A popover is open on this metric: adds per-core data and top processes.
    Hover(MetricId),
    /// The window is open: adds the full process table and every sensor.
    Window,
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Panel => f.write_str("panel"),
            Self::Hover(m) => write!(f, "hover:{m}"),
            Self::Window => f.write_str("window"),
        }
    }
}

impl FromStr for Tier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "panel" => Ok(Self::Panel),
            "window" => Ok(Self::Window),
            _ => s
                .strip_prefix("hover:")
                .and_then(|m| m.parse().ok())
                .map(Self::Hover)
                .ok_or_else(|| format!("unknown tier {s:?}; expected panel, hover:<metric> or window")),
        }
    }
}

/// What the union of all subscriptions asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Demand {
    pub panel: bool,
    pub hover: BTreeSet<MetricId>,
    pub window: bool,
}

impl Demand {
    pub fn any(&self) -> bool {
        self.panel || self.window || !self.hover.is_empty()
    }

    /// Whether the process table must be scanned this tick.
    pub fn wants_processes(&self) -> bool {
        self.window || self.hover.contains(&MetricId::Cpu) || self.hover.contains(&MetricId::Mem)
    }

    /// Whether per-core load and frequency are needed.
    pub fn wants_cores(&self) -> bool {
        self.window || self.hover.contains(&MetricId::Cpu)
    }
}

#[derive(Debug, Clone)]
struct Sub {
    owner: String,
    tier: Tier,
}

#[derive(Debug, Default)]
pub struct Subscriptions {
    next: u32,
    subs: HashMap<u32, Sub>,
}

impl Subscriptions {
    pub fn subscribe(&mut self, owner: &str, tier: Tier) -> u32 {
        self.next = self.next.wrapping_add(1).max(1);
        while self.subs.contains_key(&self.next) {
            self.next = self.next.wrapping_add(1).max(1);
        }
        self.subs.insert(self.next, Sub { owner: owner.to_owned(), tier });
        self.next
    }

    /// Remove a handle. Only its owner may remove it.
    pub fn unsubscribe(&mut self, owner: &str, handle: u32) -> bool {
        if self.subs.get(&handle).is_some_and(|s| s.owner == owner) {
            self.subs.remove(&handle);
            true
        } else {
            false
        }
    }

    /// Drop every subscription held by a connection that left the bus.
    pub fn drop_owner(&mut self, owner: &str) -> usize {
        let before = self.subs.len();
        self.subs.retain(|_, s| s.owner != owner);
        before - self.subs.len()
    }

    pub fn owns_any(&self, owner: &str) -> bool {
        self.subs.values().any(|s| s.owner == owner)
    }

    pub fn len(&self) -> usize {
        self.subs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.subs.is_empty()
    }

    pub fn demand(&self) -> Demand {
        let mut d = Demand::default();
        for s in self.subs.values() {
            match s.tier {
                Tier::Panel => d.panel = true,
                Tier::Hover(m) => {
                    d.hover.insert(m);
                }
                Tier::Window => d.window = true,
            }
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tiers() {
        assert_eq!("panel".parse::<Tier>().unwrap(), Tier::Panel);
        assert_eq!("window".parse::<Tier>().unwrap(), Tier::Window);
        assert_eq!("hover:cpu".parse::<Tier>().unwrap(), Tier::Hover(MetricId::Cpu));
        assert!("hover:x".parse::<Tier>().is_err());
        assert!("nope".parse::<Tier>().is_err());
        assert_eq!(Tier::Hover(MetricId::Net).to_string(), "hover:net");
        assert_eq!(Tier::Panel.to_string(), "panel");
        assert_eq!(Tier::Window.to_string(), "window");
    }

    #[test]
    fn lifecycle() {
        let mut s = Subscriptions::default();
        assert!(!s.demand().any());
        let a = s.subscribe(":1.10", Tier::Panel);
        let b = s.subscribe(":1.10", Tier::Hover(MetricId::Cpu));
        let c = s.subscribe(":1.20", Tier::Window);
        assert_ne!(a, b);
        let d = s.demand();
        assert!(d.panel && d.window && d.wants_processes() && d.wants_cores());
        assert!(!s.unsubscribe(":1.20", a), "only the owner may unsubscribe");
        assert!(s.unsubscribe(":1.10", b));
        assert!(s.owns_any(":1.20"));
        assert_eq!(s.drop_owner(":1.20"), 1);
        assert!(!s.owns_any(":1.20"));
        let d = s.demand();
        assert!(d.panel && !d.window && !d.wants_processes());
        assert_eq!(s.len(), 1);
        assert!(!s.unsubscribe(":1.10", c));
        s.unsubscribe(":1.10", a);
        assert!(s.is_empty());
        let mut hm = Demand::default();
        hm.hover.insert(MetricId::Mem);
        assert!(hm.wants_processes() && !hm.wants_cores() && hm.any());
    }
}
