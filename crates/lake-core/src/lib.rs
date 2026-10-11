//! lake-core: everything the Lake Levels site calculates, in plain Rust with no
//! dependencies. The data builder (`lake-build`) and the web page (through
//! `lake-web`, compiled to WebAssembly) both use it, so the Excel file and the
//! page always agree.

pub mod calc;
pub mod csv;
pub mod dates;
pub mod json;
pub mod series;

pub use calc::{month_table, projection, MonthTable, Projection};
pub use series::{Level, Loader, Series, Source};
