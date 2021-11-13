#[macro_use]
extern crate log;

mod errors;
mod github;
mod http;
mod manager;
mod release;
mod state;

pub use errors::Error;
pub use github::GitHubSource;
pub use manager::Manager;
pub use release::{Release, ReleaseVariant};
pub use state::*;
