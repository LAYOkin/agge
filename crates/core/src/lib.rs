mod graph;
mod index;
mod model;
pub mod pixel;
pub mod script;
pub mod store;

pub use graph::Direction;
pub use index::*;
pub use model::*;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn fail(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(message.into()).into()
}
