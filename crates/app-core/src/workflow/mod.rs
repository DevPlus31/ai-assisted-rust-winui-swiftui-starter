//! Automated workflows: rules → reviewable plan → apply → undo.

mod apply;
mod destinations;
mod plan;
mod rule;

pub use apply::{AppliedMove, ApplyReport, Batch, Mode, apply, undo};
pub use destinations::Destinations;
pub use plan::{Plan, PlannedMove, RENAME_MIN_CONFIDENCE, plan};
pub use rule::{Condition, Destination, Rule, all_match, default_rules};
