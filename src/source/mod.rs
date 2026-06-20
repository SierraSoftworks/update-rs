use crate::{Error, Release, ReleaseVariant};
use std::io::Write;

pub mod github;

pub use github::GitHubSource;

/// A source of application releases which the [`UpdateManager`](crate::UpdateManager)
/// can list and download binaries from.
///
/// The crate ships [`GitHubSource`] which fetches releases from a GitHub
/// repository's releases API, but you can implement this trait yourself to pull
/// updates from any other location (a custom HTTP endpoint, an S3 bucket, a
/// local directory, ...).
///
/// A source is responsible for selecting the asset relevant to the running
/// platform: each [`Release`] it returns from [`get_releases`](Source::get_releases)
/// should carry, in [`Release::variant`], the asset that should be installed here
/// (for [`GitHubSource`], the asset matching the configured glob pattern) or
/// `None` if there is none. The manager downloads [`Release::get_variant`].
///
/// Implementations must be cheap to construct via [`Default`] (the manager uses
/// it for its generic-parameter default) and must be `Send + Sync` so the
/// manager can be used from async contexts.
#[async_trait::async_trait]
pub trait Source: Default + std::fmt::Debug + Send + Sync {
    /// List the releases which are available from this source, each carrying the
    /// asset relevant to the running platform in [`Release::variant`].
    async fn get_releases(&self) -> Result<Vec<Release>, Error>;

    /// Download the binary for a specific release variant, writing it to the
    /// provided sink.
    async fn get_binary<W: Write + Send>(
        &self,
        release: &Release,
        variant: &ReleaseVariant,
        into: &mut W,
    ) -> Result<(), Error>;
}
