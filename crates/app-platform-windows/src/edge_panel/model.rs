//! Platform-neutral behaviour of the Windows Edge Dock (tested on any OS).
//!
//! Expanding grows the window first, then reveals content; collapsing
//! hides content after a short grace period, then shrinks the window once
//! the fade has finished. This order keeps empty window space from ever
//! blocking clicks on what is underneath.

use std::time::Duration;

use app_core::edge::EdgePlacement;

/// Grace period so brushing past the pill doesn't flicker it.
pub const HOVER_OUT_DELAY: Duration = Duration::from_millis(250);
/// Matches the content fade before the window shrinks.
pub const FADE: Duration = Duration::from_millis(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // independent UI flags, not an encoded mode
pub struct EdgeDockModel {
    pub placement: EdgePlacement,
    /// Window is at its expanded size.
    pub window_expanded: bool,
    /// Content of the expanded panel is visible.
    pub content_visible: bool,
    hovering: bool,
    dragging: bool,
    /// Invalidates timers scheduled before the latest interaction.
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeEvent {
    PointerEntered,
    PointerExited,
    DragEntered,
    DragLeft,
    Dropped,
    /// The hover-out grace period ended (`generation` at scheduling time).
    HoverOutElapsed(u64),
    /// The content fade ended.
    FadeElapsed(u64),
    PlacementChanged(EdgePlacement),
}

/// Side effects for the shell to perform, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Resize the window to the expanded frame.
    GrowWindow,
    /// Resize the window to the collapsed frame.
    ShrinkWindow,
    /// Re-position for a new placement (collapsed).
    Relayout,
    HideWindow,
    /// Start a timer that delivers `HoverOutElapsed(generation)`.
    StartHoverOutTimer(u64),
    /// Start a timer that delivers `FadeElapsed(generation)`.
    StartFadeTimer(u64),
}

impl EdgeDockModel {
    pub fn new(placement: EdgePlacement) -> Self {
        Self {
            placement,
            ..Self::default()
        }
    }

    pub fn handle(&self, event: EdgeEvent) -> (Self, Vec<Effect>) {
        let mut next = *self;
        let effects = match event {
            EdgeEvent::PointerEntered => {
                next.hovering = true;
                next.expand()
            }
            EdgeEvent::DragEntered => {
                next.dragging = true;
                next.expand()
            }
            EdgeEvent::PointerExited => {
                next.hovering = false;
                next.schedule_collapse()
            }
            EdgeEvent::DragLeft | EdgeEvent::Dropped => {
                next.dragging = false;
                next.schedule_collapse()
            }
            EdgeEvent::HoverOutElapsed(generation)
                if generation == self.generation && !self.engaged() =>
            {
                next.content_visible = false;
                vec![Effect::StartFadeTimer(generation)]
            }
            EdgeEvent::FadeElapsed(generation)
                if generation == self.generation && !self.engaged() && self.window_expanded =>
            {
                next.window_expanded = false;
                vec![Effect::ShrinkWindow]
            }
            EdgeEvent::HoverOutElapsed(_) | EdgeEvent::FadeElapsed(_) => Vec::new(),
            EdgeEvent::PlacementChanged(placement) => {
                next = Self {
                    placement,
                    generation: self.generation + 1,
                    ..Self::default()
                };
                if placement == EdgePlacement::Off {
                    vec![Effect::HideWindow]
                } else {
                    vec![Effect::Relayout]
                }
            }
        };
        (next, effects)
    }

    fn engaged(&self) -> bool {
        self.hovering || self.dragging
    }

    fn expand(&mut self) -> Vec<Effect> {
        self.generation += 1; // cancels pending collapse timers
        let mut effects = Vec::new();
        if !self.window_expanded {
            self.window_expanded = true;
            effects.push(Effect::GrowWindow);
        }
        self.content_visible = true;
        effects
    }

    fn schedule_collapse(&mut self) -> Vec<Effect> {
        if self.engaged() || !self.window_expanded {
            return Vec::new();
        }
        self.generation += 1;
        vec![Effect::StartHoverOutTimer(self.generation)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(model: EdgeDockModel, events: &[EdgeEvent]) -> (EdgeDockModel, Vec<Effect>) {
        events
            .iter()
            .fold((model, Vec::new()), |(model, mut all), event| {
                let (next, effects) = model.handle(*event);
                all.extend(effects);
                (next, all)
            })
    }

    fn timer_generation(effects: &[Effect]) -> u64 {
        effects
            .iter()
            .rev()
            .find_map(|e| match e {
                Effect::StartHoverOutTimer(g) | Effect::StartFadeTimer(g) => Some(*g),
                _ => None,
            })
            .expect("a timer was scheduled")
    }

    #[test]
    fn hover_grows_the_window_before_showing_content() {
        let (model, effects) =
            EdgeDockModel::new(EdgePlacement::Top).handle(EdgeEvent::PointerEntered);
        assert_eq!(effects, vec![Effect::GrowWindow]);
        assert!(model.window_expanded && model.content_visible);
    }

    #[test]
    fn leaving_fades_then_shrinks_after_the_grace_period() {
        let (model, effects) = run(
            EdgeDockModel::new(EdgePlacement::Top),
            &[EdgeEvent::PointerEntered, EdgeEvent::PointerExited],
        );
        let generation = timer_generation(&effects);
        assert!(model.content_visible, "still open during the grace period");

        let (model, effects) = model.handle(EdgeEvent::HoverOutElapsed(generation));
        assert!(!model.content_visible);
        assert!(model.window_expanded, "window shrinks only after the fade");
        assert_eq!(effects, vec![Effect::StartFadeTimer(generation)]);

        let (model, effects) = model.handle(EdgeEvent::FadeElapsed(generation));
        assert_eq!(effects, vec![Effect::ShrinkWindow]);
        assert!(!model.window_expanded);
    }

    #[test]
    fn coming_back_cancels_a_pending_collapse() {
        let (model, effects) = run(
            EdgeDockModel::new(EdgePlacement::Left),
            &[EdgeEvent::PointerEntered, EdgeEvent::PointerExited],
        );
        let stale = timer_generation(&effects);
        let (model, effects) = model.handle(EdgeEvent::PointerEntered);
        assert!(effects.is_empty(), "already grown");

        let (model, effects) = model.handle(EdgeEvent::HoverOutElapsed(stale));
        assert!(effects.is_empty());
        assert!(model.content_visible && model.window_expanded);
    }

    #[test]
    fn dragging_keeps_it_open_even_when_the_pointer_leaves() {
        let (model, effects) = run(
            EdgeDockModel::new(EdgePlacement::Bottom),
            &[
                EdgeEvent::DragEntered,
                EdgeEvent::PointerEntered,
                EdgeEvent::PointerExited,
            ],
        );
        assert_eq!(effects, vec![Effect::GrowWindow]);
        assert!(model.content_visible);

        let (_, effects) = model.handle(EdgeEvent::Dropped);
        assert!(matches!(effects[..], [Effect::StartHoverOutTimer(_)]));
    }

    #[test]
    fn changing_placement_collapses_and_relayouts_or_hides() {
        let (open, _) = EdgeDockModel::new(EdgePlacement::Top).handle(EdgeEvent::PointerEntered);

        let (moved, effects) = open.handle(EdgeEvent::PlacementChanged(EdgePlacement::Right));
        assert_eq!(effects, vec![Effect::Relayout]);
        assert!(!moved.window_expanded);
        assert_eq!(moved.placement, EdgePlacement::Right);

        let (_, effects) = moved.handle(EdgeEvent::PlacementChanged(EdgePlacement::Off));
        assert_eq!(effects, vec![Effect::HideWindow]);
    }
}
