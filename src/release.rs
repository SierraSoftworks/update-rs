use semver::Version;
use std::env::consts::{ARCH, OS};

/// Describes a specific software release version, including its unique
/// identifier, changelog, version number, and the list of platform
/// variants which are available to update to.
#[derive(Debug, Clone)]
pub struct Release {
    /// The unique ID associated with this release version.
    pub id: String,

    /// The changelog describing what has been changed in this release.
    pub changelog: String,

    /// A semver version number for this release.
    pub version: Version,

    /// The list of platform variants that this release is available for.
    pub variants: Vec<ReleaseVariant>,
}

impl Release {
    /// Gets a specific release variant from this release.
    /// 
    /// This method is intended to be used when you know which variant you are looking for
    /// (platform + architecture) and you want to get the specific instance of that
    /// variant defined in this release.
    pub fn get_variant(&self, variant: &ReleaseVariant) -> Option<&ReleaseVariant> {
        for v in self.variants.iter() {
            if v == variant {
                return Some(&v);
            }
        }

        None
    }

    /// Gets the latest release from a list of provided releases.
    /// 
    /// This method is intended to be used to help you quickly determine which
    /// release to automatically upgrade to. It accepts a list of releases,
    /// which will usually be retrieved from the [crate::Manager] and will return a
    /// [Release] object representing the newest in the list.
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

#[derive(Debug, Clone)]
pub struct ReleaseVariant {
    pub id: String,
    pub arch: String,
    pub platform: String,
}

impl Default for ReleaseVariant {
    fn default() -> Self {
        Self {
            id: String::new(),
            platform: translate_platform(OS).to_string(),
            arch: translate_arch(ARCH).to_string(),
        }
    }
}

impl PartialEq<ReleaseVariant> for ReleaseVariant {
    fn eq(&self, other: &ReleaseVariant) -> bool {
        self.arch == other.arch && self.platform == other.platform
    }
}

fn translate_platform(platform: &str) -> &str {
    match platform {
        "macos" => "darwin",
        x => x,
    }
}

fn translate_arch(arch: &str) -> &str {
    match arch {
        "x86_64" => "amd64",
        "i686" => "386",
        "aarch64" => "arm64",
        x => x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_latest() {
        assert_eq!(Release::get_latest(vec![]), None);

        let releases = vec![
            Release {
                id: "1".to_string(),
                changelog: "".to_string(),
                version: "1.1.7".parse().unwrap(),
                variants: vec![],
            },
            Release {
                id: "0".to_string(),
                changelog: "".to_string(),
                version: "1.0.0".parse().unwrap(),
                variants: vec![],
            },
            Release {
                id: "3".to_string(),
                changelog: "".to_string(),
                version: "2.3.1".parse().unwrap(),
                variants: vec![],
            },
            Release {
                id: "2".to_string(),
                changelog: "".to_string(),
                version: "2.1.0".parse().unwrap(),
                variants: vec![],
            },
        ];

        assert_eq!(
            Release::get_latest(releases.iter()).map(|r| r.id.as_str()),
            Some("3")
        );
    }

    #[test]
    fn test_variant_equality() {
        let v1 = ReleaseVariant {
            id: "test1".to_string(),
            arch: "x86_64".to_string(),
            platform: "linux".to_string(),
        };

        let v2 = ReleaseVariant {
            id: "test2".to_string(),
            arch: "x86_64".to_string(),
            platform: "linux".to_string(),
        };

        let v3 = ReleaseVariant {
            id: "test3".to_string(),
            arch: "x86_64".to_string(),
            platform: "windows".to_string(),
        };

        assert_eq!(v1, v2);
        assert_ne!(v1, v3);
        assert_ne!(v2, v3);
    }

    #[test]
    fn test_get_variants() {
        let release = Release {
            id: "test".to_string(),
            changelog: "...".to_string(),
            version: "1.0.0".parse().unwrap(),
            variants: vec![
                ReleaseVariant {
                    id: "windows-x64".to_string(),
                    arch: "x86_64".to_string(),
                    platform: "windows".to_string(),
                },
                ReleaseVariant {
                    id: "linux-x64".to_string(),
                    arch: "x86_64".to_string(),
                    platform: "linux".to_string(),
                },
            ],
        };

        let v1 = ReleaseVariant {
            id: "test1".to_string(),
            arch: "x86_64".to_string(),
            platform: "linux".to_string(),
        };

        let v = release.get_variant(&v1);
        assert_eq!(
            v,
            Some(&ReleaseVariant {
                id: "linux-x64".to_string(),
                arch: "x86_64".to_string(),
                platform: "linux".to_string()
            })
        );
        assert_eq!(v.unwrap().id, "linux-x64");
    }
}
