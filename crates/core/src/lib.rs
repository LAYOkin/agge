mod graph;
mod index;
mod model;
pub mod pixel;
mod progress;
pub mod script;
pub mod store;

pub use graph::Direction;
pub use index::*;
pub use model::*;
pub use progress::{Progress, ProgressMigration, ProgressSchema, ProgressSlot};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn fail(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(message.into()).into()
}
