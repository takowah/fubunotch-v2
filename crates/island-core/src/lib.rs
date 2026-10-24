//! FubuNotch v2's core: the activity contract and the priority arbiter.
//! No UI and no I/O, so it can be tested on its own and kept if the UI changes.

pub mod activity;
pub mod arbiter;
pub mod settings;
pub mod daily;

pub use activity::{Action, Activity, Incoming, Invalid, Kind, Priority};
pub use arbiter::{Arbiter, View, MIN_SHOW_MS};
pub use settings::{module_info, ModuleEntry, ModuleInfo, Settings, MODULES};
