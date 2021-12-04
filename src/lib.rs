#![allow(dead_code)]
#![feature(result_copied)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(slice_ptr_len)]
#![feature(core_intrinsics)]
#![feature(exclusive_range_pattern)]
#![feature(associated_type_defaults)]
#![feature(associated_type_bounds)]

mod bloom;
mod checksum;
mod db;
pub mod debra;
mod entry;
mod error;
mod format;
mod iterator;
mod iterator_trait;
mod levels;
mod memtable;
mod ops;
mod opt;
mod table;
mod util;
mod value;
mod value_log;
mod wal;
//pub mod alloc;

pub use format::{get_ts, key_with_ts};
pub use opt::ChecksumVerificationMode;
pub use opt::Options as TableOptions;
pub use table::builder::Builder as TableBuilder;
pub use table::Table;
pub use value::Value;

pub use db::{Agate, AgateOptions};
pub use error::{Error, Result};
pub use iterator_trait::AgateIterator;
pub use skiplist::Skiplist;

#[macro_use]
extern crate lazy_static;
