use semver::Version;

/// A single release published by a [`Source`](crate::Source), along with the
/// binary (variant) selected for it by the source's configured asset selection.
#[derive(Debug, Clone)]
pub struct Release {
    /// The release identifier, usually the Git tag it was published under
    /// (e.g. `v1.2.3`). This is what gets embedded in the temporary file name
    /// and used to construct download URLs.
    pub id: String,
    /// The release notes / changelog associated with the release.
    pub changelog: String,
    /// The parsed [semantic version](semver::Version) of the release, used to
    /// determine which release is the newest.
    pub version: Version,
    /// Whether this release is a pre-release (beta/early-access) version.
    pub prerelease: bool,
    /// The release asset selected for this platform, if any. For
    /// [`GitHubSource`](crate::GitHubSource) this is the asset whose name matches
    /// the configured glob pattern, or `None` when the release has no matching
    /// asset.
    pub variant: Option<ReleaseVariant>,
}

impl Release {
    /// The variant (asset) selected for this platform, if the release has one.
    ///
    /// The source has already resolved this to the asset matching its configured
    /// selection. Use [`is_some`](Option::is_some) to check whether a release
    /// has a usable binary before offering it as an update.
    pub fn get_variant(&self) -> Option<&ReleaseVariant> {
        self.variant.as_ref()
    }

    /// Return the release with the highest [version](Release::version) from an
    /// iterator of releases, or `None` if the iterator is empty.
    pub fn get_latest<'a, I>(releases: I) -> Option<&'a Self>
    where
        I: IntoIterator<Item = &'a Self>,
    {
        let mut latest: Option<&Self> = None;

        for r in releases {
            match latest {
                Some(lr) if r.version > lr.version => latest = Some(r),
                None => latest = Some(r),
                _ => {}
            }
        }

        latest
    }
}

impl PartialEq<Release> for Release {
    fn eq(&self, other: &Release) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Display for Release {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.prerelease {
            false => write!(f, "{}", &self.id),
            true => write!(f, "{}-beta", &self.id),
        }
    }
}

/// A downloadable binary belonging to a [`Release`] — a single release asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseVariant {
    /// The release asset's file name. For [`GitHubSource`](crate::GitHubSource)
    /// this is used directly to construct the download URL.
    pub name: String,
    /// The expected SHA-256 digest of the asset as lowercase hex, if the source
    /// provides one. When present, the source verifies the downloaded bytes
    /// against it before the update proceeds.
    pub sha256: Option<String>,
}

impl std::fmt::Display for ReleaseVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variant(name: &str) -> ReleaseVariant {
        ReleaseVariant {
            name: name.to_string(),
            sha256: None,
        }
    }

    #[test]
    fn test_get_latest() {
        assert_eq!(Release::get_latest(vec![]), None);

        let releases = [
            Release {
                id: "1".to_string(),
                changelog: "".to_string(),
                version: "1.1.7".parse().unwrap(),
                prerelease: false,
                variant: None,
            },
            Release {
                id: "0".to_string(),
                changelog: "".to_string(),
                version: "1.0.0".parse().unwrap(),
                prerelease: false,
                variant: None,
            },
            Release {
                id: "3".to_string(),
                changelog: "".to_string(),
                version: "2.3.1".parse().unwrap(),
                prerelease: false,
                variant: None,
            },
            Release {
                id: "2".to_string(),
                changelog: "".to_string(),
                version: "2.1.0".parse().unwrap(),
                prerelease: false,
                variant: None,
            },
        ];

        assert_eq!(
            Release::get_latest(releases.iter()).map(|r| r.id.as_str()),
            Some("3")
        );
    }

    #[test]
    fn test_get_variant() {
        let with = Release {
            id: "v1".to_string(),
            changelog: "".to_string(),
            version: "1.0.0".parse().unwrap(),
            prerelease: false,
            variant: Some(variant("myapp-linux-amd64")),
        };
        assert_eq!(with.get_variant(), Some(&variant("myapp-linux-amd64")));

        let without = Release {
            id: "v1".to_string(),
            changelog: "".to_string(),
            version: "1.0.0".parse().unwrap(),
            prerelease: false,
            variant: None,
        };
        assert_eq!(without.get_variant(), None);
    }
}
