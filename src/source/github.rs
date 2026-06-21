use super::Source;
#[cfg(not(feature = "tracing"))]
use log::debug;
#[cfg(feature = "tracing")]
use tracing::debug;

use crate::{Error, Release, ReleaseVariant, glob};
use futures_util::StreamExt;
use human_errors::ResultExt;
use reqwest::StatusCode;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Write;

/// The `User-Agent` header sent with every request, built from this crate's
/// version at compile time. GitHub requires a `User-Agent` on all API requests.
const USER_AGENT: &str = concat!("SierraSoftworks/update-rs v", env!("CARGO_PKG_VERSION"));

/// A [`Source`] which lists and downloads releases from a GitHub repository's
/// [releases API](https://docs.github.com/en/rest/releases).
///
/// The asset to download is selected by matching a **glob pattern** against each
/// release's asset file names, so your project can name its release assets
/// however it likes — there is no required naming scheme. The pattern is the
/// second argument to [`new`](GitHubSource::new):
///
/// ```
/// use std::env::consts::{ARCH, EXE_SUFFIX, OS};
/// use update_rs::GitHubSource;
///
/// let source = GitHubSource::new(
///     "sierrasoftworks/git-tool",
///     format!("git-tool-{OS}-{ARCH}{EXE_SUFFIX}"),
/// )
/// .with_release_tag_prefix("v");
/// ```
///
/// The [`naming`](crate::naming) helpers build common patterns for you, e.g.
/// [`naming::go`](crate::naming::go) (`git-tool-linux-amd64`) or
/// [`naming::rust`](crate::naming::rust) (`git-tool-x86_64-unknown-linux-gnu`).
///
/// The pattern must match the asset's **whole** file name (it is anchored at
/// both ends), so an exact name won't accidentally select a `.sha256` checksum
/// or `.sig` sidecar. It supports `*` (any sequence) and `?` (single character)
/// wildcards; every other character matches literally. `release_tag_prefix` is
/// stripped from each Git tag before it is parsed as a
/// [semantic version](semver::Version), and tags that don't parse afterwards are
/// ignored.
///
/// When GitHub reports a SHA-256 [digest](https://docs.github.com/en/rest/releases/assets)
/// for an asset, the downloaded bytes are verified against it before the update
/// proceeds, so a corrupted or tampered download is rejected.
pub struct GitHubSource {
    github_endpoint: String,
    github_api: String,
    repo: String,
    asset_pattern: String,
    release_tag_prefix: String,

    client: reqwest::Client,
}

impl GitHubSource {
    /// Create a source for the `owner/name` `repo` (e.g.
    /// `"sierrasoftworks/git-tool"`), selecting the asset to download with the
    /// glob `asset_pattern` (e.g. `"git-tool-linux-amd64"` or `"*-linux-amd64"`).
    ///
    /// See the [`naming`](crate::naming) module for helpers that build a pattern
    /// for the current platform.
    pub fn new(repo: impl Into<String>, asset_pattern: impl Into<String>) -> Self {
        Self {
            github_endpoint: "https://github.com".to_string(),
            github_api: "https://api.github.com".to_string(),
            repo: repo.into(),
            asset_pattern: asset_pattern.into(),
            release_tag_prefix: String::new(),

            client: reqwest::Client::new(),
        }
    }

    /// Strip `prefix` from each release's Git tag before parsing it as a
    /// [semantic version](semver::Version) (e.g. `"v"` for `vX.Y.Z` tags).
    pub fn with_release_tag_prefix(mut self, prefix: &str) -> Self {
        self.release_tag_prefix = prefix.to_string();
        self
    }

    /// Override the GitHub web (`web`) and API (`api`) endpoints. This is
    /// primarily useful for pointing the source at a GitHub Enterprise instance
    /// or a mock server in tests. Trailing slashes are trimmed.
    pub fn with_github_endpoints(mut self, web: &str, api: &str) -> Self {
        self.github_endpoint = web.trim_end_matches('/').to_string();
        self.github_api = api.trim_end_matches('/').to_string();
        self
    }
}

impl Default for GitHubSource {
    fn default() -> Self {
        GitHubSource::new("", "*")
    }
}

impl std::fmt::Debug for GitHubSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GitHub - {} ({})", &self.repo, &self.asset_pattern)
    }
}

#[async_trait::async_trait]
impl Source for GitHubSource {
    async fn get_releases(&self) -> Result<Vec<Release>, Error> {
        let uri = format!("{}/repos/{}/releases", self.github_api, self.repo);
        debug!("Making GET request to {} to check for new releases.", uri);

        let resp = self.get(&uri).await?;
        debug!(
            "Received HTTP {} from GitHub when requesting releases.",
            resp.status()
        );

        match resp.status() {
            StatusCode::OK => {
                let releases: Vec<GitHubRelease> = resp.json().await.wrap_system_err(
                    "Unable to parse the response from the GitHub releases API.",
                    &["The GitHub API may be unavailable or may have changed in an incompatible way; please report this issue if it persists."],
                )?;

                debug!("Received {} releases from GitHub.", releases.len());
                Ok(self.get_releases_from_response(releases))
            }
            StatusCode::NOT_FOUND => Err(human_errors::user(
                "GitHub returned a 404 Not Found when listing the releases for this repository.",
                &[
                    "Check that the repository exists and is public, and that the update manager is configured with the correct 'owner/name' repository identifier.",
                ],
            )),
            StatusCode::TOO_MANY_REQUESTS | StatusCode::FORBIDDEN => Err(human_errors::user(
                "GitHub has rate limited requests from your IP address.",
                &["Please wait until GitHub removes this rate limit before trying again."],
            )),
            status => {
                let body = resp.text().await.unwrap_or_default();
                Err(human_errors::wrap_system(
                    body,
                    format!(
                        "Received an HTTP {status} response from GitHub when listing the available releases."
                    ),
                    &[
                        "Please read the error message below and decide if there is something you can do to fix the problem, or report the issue.",
                    ],
                ))
            }
        }
    }

    async fn get_binary<W: Write + Send>(
        &self,
        release: &Release,
        variant: &ReleaseVariant,
        into: &mut W,
    ) -> Result<(), Error> {
        let uri = format!(
            "{}/{}/releases/download/{}/{}",
            self.github_endpoint, self.repo, release.id, variant.name
        );

        self.download_to_file(&uri, variant.sha256.as_deref(), into)
            .await
    }
}

impl GitHubSource {
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self)))]
    async fn get(&self, uri: &str) -> Result<reqwest::Response, Error> {
        self.client
            .get(uri)
            .header("User-Agent", USER_AGENT)
            .send()
            .await
            .wrap_system_err(
                format!("Failed to make a request to '{uri}'."),
                &["Check your network connection and try again, or report the issue if it persists."],
            )
    }

    fn get_releases_from_response(&self, releases: Vec<GitHubRelease>) -> Vec<Release> {
        let mut output: Vec<Release> = Vec::with_capacity(releases.len());

        for r in releases {
            if !r.tag_name.starts_with(&self.release_tag_prefix) {
                continue;
            }

            match r.tag_name[self.release_tag_prefix.len()..].parse() {
                Ok(version) => {
                    debug!("Found release '{}'.", r.tag_name);
                    output.push(Release {
                        id: r.tag_name.clone(),
                        changelog: r.body.clone(),
                        version,
                        prerelease: r.prerelease,
                        variant: self.get_variant_from_response(&r),
                    })
                }
                Err(_) => {
                    debug!(
                        "Skipping release '{}' because it is not a valid SemVer version (adjust the release tag prefix to fix this).",
                        &r.tag_name
                    );
                }
            }
        }

        output
    }

    /// Select the first release asset whose name matches the configured glob
    /// pattern, capturing its SHA-256 digest if GitHub reported one.
    fn get_variant_from_response(&self, release: &GitHubRelease) -> Option<ReleaseVariant> {
        release
            .assets
            .iter()
            .find(|a| glob::matches(&self.asset_pattern, &a.name))
            .map(|a| ReleaseVariant {
                name: a.name.clone(),
                sha256: a.digest.as_deref().and_then(parse_sha256_digest),
            })
    }

    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, into)))]
    async fn download_to_file<W: Write + Send>(
        &self,
        uri: &str,
        expected_sha256: Option<&str>,
        into: &mut W,
    ) -> Result<(), Error> {
        let resp = self.get(uri).await?;

        match resp.status() {
            StatusCode::OK => {
                let mut hasher = Sha256::new();
                let mut stream = resp.bytes_stream();

                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.wrap_user_err(
                        format!("Failed to download the update from '{uri}'."),
                        &["Check your network connection and try again, or report the issue if it persists."],
                    )?;
                    hasher.update(&chunk);
                    into.write_all(&chunk).wrap_user_err(
                        format!("Could not write data downloaded from '{uri}' to disk due to an OS-level error."),
                        &["Check that this tool has permission to create and write to this file and that the parent directory exists."],
                    )?;
                }

                match expected_sha256 {
                    Some(expected) => {
                        let actual = to_hex(hasher.finalize().as_slice());
                        if actual.eq_ignore_ascii_case(expected) {
                            debug!("Verified the downloaded update against its SHA-256 digest.");
                        } else {
                            return Err(human_errors::user(
                                format!(
                                    "The update downloaded from '{uri}' failed its integrity check (expected SHA-256 {expected}, got {actual})."
                                ),
                                &[
                                    "The download may have been corrupted in transit or tampered with. Please try the update again, and report the issue if it keeps happening.",
                                ],
                            ));
                        }
                    }
                    None => {
                        debug!(
                            "No SHA-256 digest was reported for this asset; skipping the integrity check."
                        );
                    }
                }

                Ok(())
            }
            StatusCode::NOT_FOUND => Err(human_errors::user(
                format!("GitHub returned a 404 Not Found when downloading '{uri}'."),
                &[
                    "This release variant may not be available for your platform, or the release may have been removed.",
                ],
            )),
            StatusCode::TOO_MANY_REQUESTS | StatusCode::FORBIDDEN => Err(human_errors::user(
                "GitHub has rate limited requests from your IP address.",
                &["Please wait until GitHub removes this rate limit before trying again."],
            )),
            status => {
                let body = resp.text().await.unwrap_or_default();
                Err(human_errors::wrap_system(
                    body,
                    format!(
                        "Received an HTTP {status} response from GitHub when downloading the update ({uri})."
                    ),
                    &[
                        "Please read the error message below and decide if there is something you can do to fix the problem, or report the issue.",
                    ],
                ))
            }
        }
    }
}

/// Parse a GitHub asset `digest` field (e.g. `"sha256:abc..."`) into its
/// lowercase hex SHA-256, returning `None` for absent or unsupported algorithms.
fn parse_sha256_digest(digest: &str) -> Option<String> {
    digest
        .strip_prefix("sha256:")
        .map(|hex| hex.trim().to_ascii_lowercase())
}

/// Encode bytes as lowercase hexadecimal.
fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    #[allow(dead_code)]
    pub name: String,
    pub tag_name: String,
    pub body: String,
    pub prerelease: bool,
    pub assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    pub name: String,
    #[serde(default)]
    pub digest: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const RELEASES_JSON: &str = r#"[
        {
            "name": "Version 2.0.0",
            "tag_name": "v2.0.0",
            "body": "Example Release",
            "prerelease": false,
            "assets": [
                { "name": "update-windows-amd64.exe" },
                { "name": "update-linux-amd64" },
                { "name": "update-darwin-amd64" }
            ]
        }
    ]"#;

    fn source_for(server: &MockServer, pattern: &str) -> GitHubSource {
        GitHubSource::new("sierrasoftworks/update-rs", pattern)
            .with_github_endpoints(&server.uri(), &server.uri())
            .with_release_tag_prefix("v")
    }

    fn sha256_hex(data: &[u8]) -> String {
        to_hex(Sha256::digest(data).as_slice())
    }

    fn releases_json(asset: &str, digest: Option<&str>) -> String {
        let digest_field = match digest {
            Some(d) => format!(r#", "digest": "{d}""#),
            None => String::new(),
        };
        format!(
            r#"[{{"name":"Version 2.0.0","tag_name":"v2.0.0","body":"Example Release","prerelease":false,"assets":[{{"name":"{asset}"{digest_field}}}]}}]"#
        )
    }

    #[tokio::test]
    async fn test_get_releases_selects_matching_asset() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/sierrasoftworks/update-rs/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RELEASES_JSON))
            .mount(&server)
            .await;

        let source = source_for(&server, "update-linux-amd64");
        let releases = source.get_releases().await.unwrap();

        assert_eq!(releases.len(), 1);
        let release = &releases[0];
        assert_eq!(release.id, "v2.0.0");
        assert_eq!(release.version.to_string(), "2.0.0");
        assert!(!release.prerelease);
        assert_ne!(release.changelog, "");

        // Only the matching asset becomes the variant.
        assert!(release.get_variant().is_some());
        let variant = release.get_variant().unwrap();
        assert_eq!(variant.name, "update-linux-amd64");
        // No digest in this fixture.
        assert!(variant.sha256.is_none());
    }

    #[tokio::test]
    async fn test_get_releases_glob_pattern() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/sierrasoftworks/update-rs/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RELEASES_JSON))
            .mount(&server)
            .await;

        // A glob that matches a single asset regardless of host platform.
        let source = source_for(&server, "*-windows-amd64.exe");
        let releases = source.get_releases().await.unwrap();

        assert_eq!(
            releases[0].get_variant().unwrap().name,
            "update-windows-amd64.exe"
        );
    }

    #[tokio::test]
    async fn test_get_releases_no_match() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/sierrasoftworks/update-rs/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RELEASES_JSON))
            .mount(&server)
            .await;

        let source = source_for(&server, "update-freebsd-amd64");
        let releases = source.get_releases().await.unwrap();

        assert_eq!(releases.len(), 1);
        assert!(releases[0].get_variant().is_none());
    }

    #[tokio::test]
    async fn test_get_releases_ignores_sidecar_files() {
        let server = MockServer::start().await;
        let body = r#"[{
            "name": "Version 2.0.0",
            "tag_name": "v2.0.0",
            "body": "Example Release",
            "prerelease": false,
            "assets": [
                { "name": "update-linux-amd64.sha256" },
                { "name": "update-linux-amd64.sig" },
                { "name": "update-linux-amd64" }
            ]
        }]"#;
        Mock::given(method("GET"))
            .and(path("/repos/sierrasoftworks/update-rs/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        // An exact pattern must select the binary, not its checksum/signature
        // sidecars, even when they are listed first.
        let source = source_for(&server, "update-linux-amd64");
        let releases = source.get_releases().await.unwrap();

        assert_eq!(
            releases[0].get_variant().unwrap().name,
            "update-linux-amd64"
        );
    }

    #[tokio::test]
    async fn test_download() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/sierrasoftworks/update-rs/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RELEASES_JSON))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/sierrasoftworks/update-rs/releases/download/v2.0.0/update-linux-amd64",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string("example update content"))
            .mount(&server)
            .await;

        let source = source_for(&server, "update-linux-amd64");
        let releases = source.get_releases().await.unwrap();
        let latest = Release::get_latest(releases.iter()).unwrap();
        let variant = latest.get_variant().unwrap();

        let mut target = Sink::new();
        source
            .get_binary(latest, variant, &mut target)
            .await
            .unwrap();

        assert!(target.len() > 0);
    }

    #[tokio::test]
    async fn test_download_verifies_matching_sha256() {
        let body = "example update content";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/sierrasoftworks/update-rs/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string(releases_json(
                "update-linux-amd64",
                Some(&format!("sha256:{}", sha256_hex(body.as_bytes()))),
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/sierrasoftworks/update-rs/releases/download/v2.0.0/update-linux-amd64",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let source = source_for(&server, "update-linux-amd64");
        let releases = source.get_releases().await.unwrap();
        let latest = Release::get_latest(releases.iter()).unwrap();
        let variant = latest.get_variant().unwrap();
        assert!(
            variant.sha256.is_some(),
            "the digest should have been parsed from the API response"
        );

        let mut target = Sink::new();
        source
            .get_binary(latest, variant, &mut target)
            .await
            .expect("a matching digest should pass verification");
        assert!(target.len() > 0);
    }

    #[tokio::test]
    async fn test_download_rejects_bad_sha256() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/sierrasoftworks/update-rs/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string(releases_json(
                "update-linux-amd64",
                Some(&format!("sha256:{}", "0".repeat(64))),
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/sierrasoftworks/update-rs/releases/download/v2.0.0/update-linux-amd64",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string("the actual bytes"))
            .mount(&server)
            .await;

        let source = source_for(&server, "update-linux-amd64");
        let releases = source.get_releases().await.unwrap();
        let latest = Release::get_latest(releases.iter()).unwrap();
        let variant = latest.get_variant().unwrap();

        let mut target = Sink::new();
        let err = source
            .get_binary(latest, variant, &mut target)
            .await
            .expect_err("a mismatched digest must fail the update");
        assert!(
            err.to_string().contains("integrity check"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_parse_sha256_digest() {
        assert_eq!(parse_sha256_digest("sha256:ABCdef"), Some("abcdef".into()));
        assert_eq!(parse_sha256_digest("sha512:abc"), None);
        assert_eq!(parse_sha256_digest(""), None);
    }

    struct Sink {
        length: Arc<Mutex<usize>>,
    }

    impl Sink {
        fn new() -> Self {
            Self {
                length: Arc::new(Mutex::new(0)),
            }
        }

        fn len(&self) -> usize {
            *self.length.lock().unwrap()
        }
    }

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            *self.length.lock().unwrap() += buf.len();
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
