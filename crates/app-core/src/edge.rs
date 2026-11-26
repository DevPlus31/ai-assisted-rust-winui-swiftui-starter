//! Layout for the Edge Dock: a pill attached to a screen edge that expands
//! into a drop panel. Pure geometry, shared by the shells and unit-tested.
//!
//! Coordinates are top-left origin, y growing downwards (Windows; the
//! macOS shell flips them). Sizes are logical pixels; the shell multiplies
//! by its display scale.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum EdgePlacement {
    /// Top centre (wraps the notch on Macs that have one).
    #[default]
    Top,
    Left,
    Right,
    /// Just above the Dock / taskbar.
    Bottom,
    Off,
}

impl EdgePlacement {
    pub const ALL: [Self; 5] = [Self::Top, Self::Left, Self::Right, Self::Bottom, Self::Off];

    pub fn label(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Left => "Left edge",
            Self::Right => "Right edge",
            Self::Bottom => "Bottom (above taskbar)",
            Self::Off => "Off",
        }
    }

    /// Next placement, for a simple "cycle" control.
    #[must_use]
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }
    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }
    pub fn contains(&self, other: &Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }
    /// Scales position and size (logical → physical pixels).
    #[must_use]
    pub fn scaled(&self, factor: f64) -> Self {
        Self::new(
            self.x * factor,
            self.y * factor,
            self.width * factor,
            self.height * factor,
        )
    }
}

/// What the layout needs to know about the target display (logical px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayMetrics {
    pub bounds: Rect,
    /// Excludes the menu bar, Dock or taskbar.
    pub work_area: Rect,
    /// Height of the notch strip; 0 without a notch.
    pub notch_height: f64,
    /// Width of the camera housing, `None` without a notch.
    pub notch_width: Option<f64>,
}

pub const EXPANDED: (f64, f64) = (380.0, 196.0);
pub const SIDE_EXPANDED: (f64, f64) = (300.0, 250.0);
pub const PILL: (f64, f64) = (210.0, 30.0);
pub const SIDE_TAB: (f64, f64) = (12.0, 140.0);
pub const NOTCH_WING: f64 = 54.0;
pub const EDGE_GAP: f64 = 6.0;
/// Corner radius of the expanded panel.
pub const RADIUS: f64 = 22.0;

/// Window frame for `placement`, or `None` when the Edge Dock is off.
/// Collapsed and expanded frames share the attached edge, so the panel
/// grows out of the pill.
pub fn frame(placement: EdgePlacement, expanded: bool, display: &DisplayMetrics) -> Option<Rect> {
    match placement {
        EdgePlacement::Off => None,
        EdgePlacement::Top => Some(top(expanded, display)),
        EdgePlacement::Left => Some(side(true, expanded, display)),
        EdgePlacement::Right => Some(side(false, expanded, display)),
        EdgePlacement::Bottom => Some(bottom(expanded, display)),
    }
}

fn top(expanded: bool, display: &DisplayMetrics) -> Rect {
    let notch = display.notch_width.filter(|_| display.notch_height > 0.0);
    let (collapsed_w, collapsed_h) = match notch {
        Some(width) => (width + 2.0 * NOTCH_WING, display.notch_height),
        None => PILL,
    };
    let (w, h) = if expanded {
        (
            EXPANDED.0.max(collapsed_w),
            EXPANDED.1
                + if notch.is_some() {
                    display.notch_height
                } else {
                    0.0
                },
        )
    } else {
        (collapsed_w, collapsed_h)
    };
    let top = if notch.is_some() {
        display.bounds.y
    } else {
        display.work_area.y
    };
    Rect::new(display.bounds.center_x() - w / 2.0, top, w, h)
}

fn side(leading: bool, expanded: bool, display: &DisplayMetrics) -> Rect {
    let (w, h) = if expanded { SIDE_EXPANDED } else { SIDE_TAB };
    let area = display.work_area;
    let x = if leading { area.x } else { area.right() - w };
    Rect::new(x, area.center_y() - h / 2.0, w, h)
}

fn bottom(expanded: bool, display: &DisplayMetrics) -> Rect {
    let (w, h) = if expanded { EXPANDED } else { PILL };
    let area = display.work_area;
    Rect::new(
        area.center_x() - w / 2.0,
        area.bottom() - EDGE_GAP - h,
        w,
        h,
    )
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // layout values are exact by construction
mod tests {
    use super::*;

    /// 1920×1080 Windows display, taskbar at the bottom (48 px).
    fn windows_display() -> DisplayMetrics {
        DisplayMetrics {
            bounds: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            work_area: Rect::new(0.0, 0.0, 1920.0, 1032.0),
            notch_height: 0.0,
            notch_width: None,
        }
    }

    /// 14-inch `MacBook` Pro (flipped to top-left origin).
    fn notched_mac() -> DisplayMetrics {
        DisplayMetrics {
            bounds: Rect::new(0.0, 0.0, 1512.0, 982.0),
            work_area: Rect::new(0.0, 32.0, 1512.0, 880.0),
            notch_height: 32.0,
            notch_width: Some(200.0),
        }
    }

    #[test]
    fn top_pill_hugs_the_top_and_grows_downwards() {
        let display = windows_display();
        let pill = frame(EdgePlacement::Top, false, &display).unwrap();
        let panel = frame(EdgePlacement::Top, true, &display).unwrap();
        assert_eq!(pill, Rect::new(855.0, 0.0, 210.0, 30.0));
        assert_eq!(panel.y, pill.y);
        assert_eq!(panel.center_x(), pill.center_x());
        assert!(panel.contains(&pill));
    }

    #[test]
    fn top_pill_wraps_a_notch() {
        let display = notched_mac();
        let pill = frame(EdgePlacement::Top, false, &display).unwrap();
        assert_eq!(pill, Rect::new(602.0, 0.0, 308.0, 32.0));
        assert_eq!(
            frame(EdgePlacement::Top, true, &display).unwrap().height,
            EXPANDED.1 + 32.0
        );
    }

    #[test]
    fn side_tabs_hug_their_edges_inside_the_work_area() {
        let display = windows_display();
        for expanded in [false, true] {
            let left = frame(EdgePlacement::Left, expanded, &display).unwrap();
            let right = frame(EdgePlacement::Right, expanded, &display).unwrap();
            assert_eq!(left.x, 0.0);
            assert_eq!(right.right(), 1920.0);
            assert!(display.work_area.contains(&left) && display.work_area.contains(&right));
        }
    }

    #[test]
    fn bottom_sits_just_above_the_taskbar_wherever_it_is() {
        let display = windows_display();
        let pill = frame(EdgePlacement::Bottom, false, &display).unwrap();
        let panel = frame(EdgePlacement::Bottom, true, &display).unwrap();
        assert_eq!(pill.bottom(), 1032.0 - EDGE_GAP);
        assert_eq!(panel.bottom(), pill.bottom());

        // Taskbar on the left: the work area moves, the pill follows.
        let left_taskbar = DisplayMetrics {
            work_area: Rect::new(62.0, 0.0, 1858.0, 1080.0),
            ..display
        };
        let pill = frame(EdgePlacement::Bottom, false, &left_taskbar).unwrap();
        assert_eq!(pill.center_x(), left_taskbar.work_area.center_x());
        assert_eq!(pill.bottom(), 1080.0 - EDGE_GAP);
    }

    #[test]
    fn off_has_no_frame_and_placements_cycle() {
        assert_eq!(frame(EdgePlacement::Off, true, &windows_display()), None);
        assert_eq!(EdgePlacement::Off.next(), EdgePlacement::Top);
        assert_eq!(EdgePlacement::Top.next(), EdgePlacement::Left);
    }

    #[test]
    fn scaling_converts_to_physical_pixels() {
        assert_eq!(
            Rect::new(10.0, 20.0, 30.0, 40.0).scaled(1.5),
            Rect::new(15.0, 30.0, 45.0, 60.0)
        );
    }
}
