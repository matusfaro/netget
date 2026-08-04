//! Utility modules

pub mod save_load;
pub mod truncate;

pub use truncate::{
    truncate_for_llm, truncate_for_log, truncate_str, truncate_with_notice, truncate_with_suffix,
};
