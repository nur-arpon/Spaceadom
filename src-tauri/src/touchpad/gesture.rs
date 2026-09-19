//! TOUCHPAD T2 — the PURE gesture state machine (docs/TOUCHPAD-BRIEF-T2.md).
//!
//! Contacts (as pad fractions) in, `GestureEvent`s out. It owns no COM, no
//! atomics, no Tauri — `touchpad::mod` drives it and turns its events into
//! actions, the `BAND_LIVE` atomic and page events. Everything here is unit
//! tested.
//!
//! The rule (T1b hardware result): exactly ONE contact down, and it LANDED
//! inside one enabled band. A contact that lands outside and drifts in never
//! arms (normal pointing is never hijacked); a landing in a corner square two
//! bands share is resolved by `corners`/`corner_rule` (`Ask` with an
//! unresolved corner = no band). We arm on the landing report and hold the
//! gesture — full hysteresis — until the finger lifts or a second contact
//! touches, because a finger that OWNS a band keeps it however far it drifts.
//! `travel` is signed displacement along the band's axis since entry, in
//! fractions of the pad, with the band's `invert` already applied.

use std::collections::HashMap;

use crate::config::schema::{CornerRule, TouchEdge, Touchpad};

/// One finger in one report, as fractions 0.0..=1.0 of the pad (X = fx along
/// the long axis, Y = fy along the short axis, origin top-left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FracContact {
    pub id: u32,
    pub fx: f32,
    pub fy: f32,
}

/// What the machine emits for `touchpad::mod` to act on.
#[derive(Clone, Debug, PartialEq)]
pub enum GestureEvent {
    /// A band went live: freeze the pointer, start the readout.
    Enter(TouchEdge),
    /// The live finger moved: `travel` is signed, `invert` applied.
    Move { edge: TouchEdge, travel: f32 },
    /// The gesture ended (lift or second contact): release the pointer.
    Exit(TouchEdge),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Idle,
    Live { edge: TouchEdge, id: u32, ex: f32, ey: f32 },
}

/// The pure machine. `cfg` is a snapshot refreshed by the caller only while
/// idle (so a settings change never rewrites a gesture mid-slide); `aspect`
/// is the pad's short/long side ratio, used so a band's physical thickness is
/// the same on every edge (the design draws a uniform band).
pub struct Gesture {
    cfg: Touchpad,
    aspect: f32,
    landed: HashMap<u32, Option<TouchEdge>>,
    phase: Phase,
}

impl Gesture {
    pub fn new(cfg: Touchpad, aspect: f32) -> Self {
        Gesture {
            cfg,
            aspect: if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 },
            landed: HashMap::new(),
            phase: Phase::Idle,
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self.phase, Phase::Live { .. })
    }

    /// Replace the config snapshot. The caller does this only while idle.
    pub fn set_config(&mut self, cfg: Touchpad, aspect: f32) {
        self.cfg = cfg;
        if aspect.is_finite() && aspect > 0.0 {
            self.aspect = aspect;
        }
    }

    /// True when `(fx,fy)` is inside `edge`'s band (which must be enabled).
    fn in_band(&self, edge: TouchEdge, fx: f32, fy: f32) -> bool {
        let b = self.cfg.band(edge);
        if !b.enabled {
            return false;
        }
        let half = b.length / 2.0;
        match edge {
            TouchEdge::Left => fx <= b.width * self.aspect && (fy - 0.5).abs() <= half,
            TouchEdge::Right => fx >= 1.0 - b.width * self.aspect && (fy - 0.5).abs() <= half,
            TouchEdge::Top => fy <= b.width && (fx - 0.5).abs() <= half,
            TouchEdge::Bottom => fy >= 1.0 - b.width && (fx - 0.5).abs() <= half,
        }
    }

    /// Every enabled band that contains the point (0, 1 or 2 — a corner).
    fn edges_at(&self, fx: f32, fy: f32) -> Vec<TouchEdge> {
        TouchEdge::ALL.iter().copied().filter(|&e| self.in_band(e, fx, fy)).collect()
    }

    /// Which band OWNS a landing at these edges — the corner resolver.
    fn resolve_landing(&self, edges: &[TouchEdge]) -> Option<TouchEdge> {
        match edges.len() {
            0 => None,
            1 => Some(edges[0]),
            _ => {
                let v = edges.iter().copied().find(|e| e.is_vertical());
                let h = edges.iter().copied().find(|e| !e.is_vertical());
                match (v, h) {
                    (Some(v), Some(h)) => self.resolve_corner(v, h),
                    _ => Some(edges[0]),
                }
            }
        }
    }

    /// The owner of the square where vertical `v` meets horizontal `h`.
    fn resolve_corner(&self, v: TouchEdge, h: TouchEdge) -> Option<TouchEdge> {
        if let Some(owner) = self.cfg.corners.get(v, h) {
            // A hand-set corner still only counts if that band is enabled.
            if self.cfg.band(owner).enabled {
                return Some(owner);
            }
        }
        match self.cfg.corner_rule {
            CornerRule::AlwaysHorizontal => Some(h),
            CornerRule::AlwaysVertical => Some(v),
            CornerRule::Ask => None,
        }
    }

    /// Signed travel from entry to `(fx,fy)` along `edge`'s axis, up/right
    /// positive, with the band's `invert` applied.
    fn travel(&self, edge: TouchEdge, ex: f32, ey: f32, fx: f32, fy: f32) -> f32 {
        let raw = if edge.is_vertical() {
            ey - fy // slide UP the pad (fy decreasing) is positive
        } else {
            fx - ex // slide RIGHT (fx increasing) is positive
        };
        if self.cfg.band(edge).invert {
            -raw
        } else {
            raw
        }
    }

    /// Feed one report. Returns the transition it caused, if any.
    pub fn feed(&mut self, contacts: &[FracContact]) -> Option<GestureEvent> {
        // Forget lifted contacts; record where each NEW one landed (once).
        self.landed.retain(|id, _| contacts.iter().any(|c| c.id == *id));
        for c in contacts {
            if !self.landed.contains_key(&c.id) {
                let owner = self.resolve_landing(&self.edges_at(c.fx, c.fy));
                self.landed.insert(c.id, owner);
            }
        }

        // The live candidate: exactly one contact, and it LANDED in a band.
        let candidate: Option<(TouchEdge, u32, f32, f32)> = if contacts.len() == 1 {
            let c = contacts[0];
            self.landed
                .get(&c.id)
                .copied()
                .flatten()
                .map(|edge| (edge, c.id, c.fx, c.fy))
        } else {
            None
        };

        match (self.phase, candidate) {
            (Phase::Idle, Some((edge, id, fx, fy))) => {
                self.phase = Phase::Live { edge, id, ex: fx, ey: fy };
                Some(GestureEvent::Enter(edge))
            }
            (Phase::Idle, None) => None,
            (Phase::Live { edge, id, ex, ey }, Some((c_edge, c_id, fx, fy)))
                if c_edge == edge && c_id == id =>
            {
                // Same finger, same band — keep sliding (drift never exits).
                let _ = (ex, ey);
                Some(GestureEvent::Move { edge, travel: self.travel(edge, ex, ey, fx, fy) })
            }
            (Phase::Live { edge, .. }, _) => {
                // Lift, second contact, or a different owning finger — end.
                self.phase = Phase::Idle;
                Some(GestureEvent::Exit(edge))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{Band, BandAction, Corners, TouchEdge};

    /// A touchpad with the named edges enabled at the default geometry.
    fn cfg(enabled: &[TouchEdge]) -> Touchpad {
        let mut t = Touchpad::default();
        for &e in enabled {
            t.band_mut(e).enabled = true;
        }
        t
    }

    fn c(id: u32, fx: f32, fy: f32) -> FracContact {
        FracContact { id, fx, fy }
    }

    #[test]
    fn a_finger_landing_in_a_band_arms_and_a_slide_reports_travel() {
        // aspect 1.0 → right band is fx >= 1 - 0.12 = 0.88.
        let mut g = Gesture::new(cfg(&[TouchEdge::Right]), 1.0);
        assert_eq!(g.feed(&[c(1, 0.97, 0.5)]), Some(GestureEvent::Enter(TouchEdge::Right)));
        assert!(g.is_live());
        // Slide UP (fy 0.5 -> 0.3): +0.2 travel (right edge, no invert).
        let ev = g.feed(&[c(1, 0.97, 0.3)]).unwrap();
        match ev {
            GestureEvent::Move { edge: TouchEdge::Right, travel } => assert!((travel - 0.2).abs() < 1e-6),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_finger_landing_outside_never_arms_even_if_it_drifts_in() {
        let mut g = Gesture::new(cfg(&[TouchEdge::Right]), 1.0);
        // Lands in the middle.
        assert_eq!(g.feed(&[c(1, 0.5, 0.5)]), None);
        // Drifts into the right band — still nothing, it did not LAND there.
        assert_eq!(g.feed(&[c(1, 0.97, 0.5)]), None);
        assert!(!g.is_live());
    }

    #[test]
    fn a_second_contact_ends_the_gesture_and_a_lift_ends_it() {
        let mut g = Gesture::new(cfg(&[TouchEdge::Left]), 1.0);
        assert_eq!(g.feed(&[c(1, 0.02, 0.5)]), Some(GestureEvent::Enter(TouchEdge::Left)));
        // Second finger → exit.
        assert_eq!(g.feed(&[c(1, 0.02, 0.5), c(2, 0.5, 0.5)]), Some(GestureEvent::Exit(TouchEdge::Left)));
        assert!(!g.is_live());
        // Re-arm, then lift → exit.
        assert_eq!(g.feed(&[c(1, 0.02, 0.5)]), Some(GestureEvent::Enter(TouchEdge::Left)));
        assert_eq!(g.feed(&[]), Some(GestureEvent::Exit(TouchEdge::Left)));
    }

    #[test]
    fn hysteresis_a_live_finger_drifting_out_of_the_band_keeps_the_gesture() {
        let mut g = Gesture::new(cfg(&[TouchEdge::Top]), 1.0);
        // Top band: fy <= 0.12.
        assert_eq!(g.feed(&[c(1, 0.5, 0.02)]), Some(GestureEvent::Enter(TouchEdge::Top)));
        // Drift well below the band — still Move, not Exit.
        let ev = g.feed(&[c(1, 0.7, 0.30)]).unwrap();
        assert!(matches!(ev, GestureEvent::Move { edge: TouchEdge::Top, .. }));
        assert!(g.is_live());
    }

    #[test]
    fn invert_flips_the_travel_sign() {
        let mut t = cfg(&[TouchEdge::Right]);
        t.right.invert = true;
        let mut g = Gesture::new(t, 1.0);
        g.feed(&[c(1, 0.97, 0.5)]);
        let ev = g.feed(&[c(1, 0.97, 0.3)]).unwrap(); // slide up
        match ev {
            GestureEvent::Move { travel, .. } => assert!((travel + 0.2).abs() < 1e-6, "inverted"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn top_edge_scrub_travel_is_positive_to_the_right() {
        let mut g = Gesture::new(cfg(&[TouchEdge::Top]), 1.0);
        g.feed(&[c(1, 0.5, 0.02)]);
        let ev = g.feed(&[c(1, 0.8, 0.02)]).unwrap();
        match ev {
            GestureEvent::Move { edge: TouchEdge::Top, travel } => assert!(travel > 0.0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_corner_landing_asks_by_default_so_no_band_owns_it() {
        // Top and right both enabled, overlapping at the top-right square.
        let mut t = cfg(&[TouchEdge::Top, TouchEdge::Right]);
        // Long bands so they actually reach the corner.
        t.top.length = 1.0;
        t.right.length = 1.0;
        let mut g = Gesture::new(t, 1.0);
        // A point in the top-right square: fx high (right band) AND fy low (top).
        assert_eq!(g.feed(&[c(1, 0.98, 0.02)]), None, "Ask → nobody owns it");
    }

    #[test]
    fn a_corner_rule_and_a_hand_set_owner_resolve_the_square() {
        let mut t = cfg(&[TouchEdge::Top, TouchEdge::Right]);
        t.top.length = 1.0;
        t.right.length = 1.0;
        t.corner_rule = CornerRule::AlwaysVertical;
        let mut g = Gesture::new(t.clone(), 1.0);
        assert_eq!(g.feed(&[c(1, 0.98, 0.02)]), Some(GestureEvent::Enter(TouchEdge::Right)));

        // A hand-set owner overrides the rule.
        let mut corners = Corners::default();
        corners.set(TouchEdge::Right, TouchEdge::Top, Some(TouchEdge::Top));
        t.corners = corners;
        let mut g2 = Gesture::new(t, 1.0);
        assert_eq!(g2.feed(&[c(1, 0.98, 0.02)]), Some(GestureEvent::Enter(TouchEdge::Top)));
    }

    /// The landing rule: a contact whose FIRST report is inside the band arms
    /// on that very report — no previous frame is needed (coordinator,
    /// 2026-09-19: the owner's 1.0.121 slides did nothing and the log could
    /// not say why; this pins the machine's half of the rule).
    #[test]
    fn a_contact_that_first_appears_inside_the_band_arms_on_that_report() {
        for edge in TouchEdge::ALL {
            let mut g = Gesture::new(cfg(&[edge]), 1.0);
            // Fresh machine, no prior frame: the landing report itself.
            let (fx, fy) = match edge {
                TouchEdge::Left => (0.05, 0.5),
                TouchEdge::Right => (0.95, 0.5),
                TouchEdge::Top => (0.5, 0.05),
                TouchEdge::Bottom => (0.5, 0.95),
            };
            assert_eq!(g.feed(&[c(7, fx, fy)]), Some(GestureEvent::Enter(edge)), "{edge:?}");
            assert!(g.is_live());
        }
    }

    /// 1.0.122: the bottom edge is a normal edge — it arms, reports travel
    /// (right positive, like the top), inverts, and exits like the others.
    #[test]
    fn the_bottom_edge_behaves_like_every_other_edge() {
        let mut g = Gesture::new(cfg(&[TouchEdge::Bottom]), 1.0);
        // Bottom band: fy >= 1 - 0.12 = 0.88, centred 80 % of X.
        assert_eq!(g.feed(&[c(1, 0.5, 0.95)]), Some(GestureEvent::Enter(TouchEdge::Bottom)));
        match g.feed(&[c(1, 0.8, 0.95)]).unwrap() {
            GestureEvent::Move { edge: TouchEdge::Bottom, travel } => assert!((travel - 0.3).abs() < 1e-6),
            other => panic!("{other:?}"),
        }
        assert_eq!(g.feed(&[]), Some(GestureEvent::Exit(TouchEdge::Bottom)));
        // Off by default, like the others; outside its length it does not arm.
        assert!(!Touchpad::default().bottom.enabled);
        let mut g2 = Gesture::new(cfg(&[TouchEdge::Bottom]), 1.0);
        assert_eq!(g2.feed(&[c(1, 0.05, 0.95)]), None, "outside the centred 80 %");
        // Inverted, right-slides read negative.
        let mut t = cfg(&[TouchEdge::Bottom]);
        t.bottom.invert = true;
        let mut g3 = Gesture::new(t, 1.0);
        g3.feed(&[c(1, 0.5, 0.95)]);
        match g3.feed(&[c(1, 0.8, 0.95)]).unwrap() {
            GestureEvent::Move { travel, .. } => assert!(travel < 0.0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_disabled_band_is_never_entered() {
        // Right present in config but not enabled.
        let mut t = Touchpad::default();
        t.right.action = BandAction::Volume;
        let mut g = Gesture::new(t, 1.0);
        assert_eq!(g.feed(&[c(1, 0.98, 0.5)]), None);
        assert_eq!(Band::for_edge(TouchEdge::Right).enabled, false);
    }
}
