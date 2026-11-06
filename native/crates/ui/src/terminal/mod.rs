//! Native terminal rendering: view model, selection and search over the core's parsed grid.

pub mod color;
pub mod search;
pub mod selection;
pub mod view;

pub use color::{ANSI_16, ansi_to_rgba, indexed_rgb};
pub use search::{MatchList, SearchHit};
pub use selection::{CellPos, Selection, char_index_for_col, char_width, display_width};
pub use view::{DisplayRow, TerminalView};
