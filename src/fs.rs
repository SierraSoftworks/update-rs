#[cfg(not(feature = "tracing"))]
use log::debug;
#[cfg(feature = "tracing")]
use tracing::debug;

use crate::{Error, Release};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(test)]
use mockall::automock;

/// The number of times a destructive filesystem operation is retried before it
/// is reported as a failure, and the delay between attempts. The original
/// application may still hold an open handle to its binary when the replace
/// phase starts, so we retry to give it time to exit.
const MAX_RETRIES: u32 = 10;
const RETRY_DELAY: Duration = Duration::from_millis(500);

pub(crate) fn default() -> Box<dyn FileSystem + Send + Sync> {
    Box::new(DefaultFileSystem {})
}

/// The filesystem operations the [`UpdateManager`](crate::UpdateManager) needs.
///
/// This is abstracted behind a trait so that the manager's state machine can be
/// unit-tested with a mock filesystem; the real implementation is
/// [`DefaultFileSystem`].
#[cfg_attr(test, automock)]
#[async_trait::async_trait]
pub trait FileSystem {
    /// Delete the file at `path`, retrying briefly if it is still locked. A
    /// missing file is treated as success.
    async fn delete_file(&self, path: &Path) -> Result<(), Error>;

    /// Copy the file at `from` over the file at `to`, retrying briefly if the
    /// destination is still locked.
    async fn copy_file(&self, from: &Path, to: &Path) -> Result<(), Error>;

    /// Compute the temporary path the new release binary should be downloaded
    /// to, derived from the application's own file name so it is unique per
    /// application and release.
    fn get_temp_app_path(&self, target_application: &Path, release: &Release) -> PathBuf;
}

#[derive(Debug)]
struct DefaultFileSystem {}

#[async_trait::async_trait]
impl FileSystem for DefaultFileSystem {
    async fn delete_file(&self, path: &Path) -> Result<(), Error> {
        let mut attempt = 0;

        loop {
            match tokio::fs::remove_file(path).await {
                Ok(_) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) if attempt >= MAX_RETRIES => {
                    return Err(human_errors::wrap_user(
                        e,
                        format!(
                            "Could not remove the file '{}' after {} retries.",
                            path.display(),
                            MAX_RETRIES
                        ),
                        &[
                            "This usually means that the application is still running in another terminal. Please exit any running instances (including shells launched by it) before trying again.",
                            "Make sure that you have write permissions on the file, or try running the application with elevated permissions.",
                        ],
                    ));
                }
                Err(_) => {
                    debug!(
                        "Failed to remove '{}' (attempt {}); retrying.",
                        path.display(),
                        attempt + 1
                    );
                    attempt += 1;
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
    }

    async fn copy_file(&self, from: &Path, to: &Path) -> Result<(), Error> {
        let mut attempt = 0;

        loop {
            match tokio::fs::copy(from, to).await {
                Ok(_) => return Ok(()),
                Err(e) if attempt >= MAX_RETRIES => {
                    return Err(human_errors::wrap_user(
                        e,
                        format!(
                            "Could not copy the new application file '{}' over the old application file '{}' after {} retries.",
                            from.display(),
                            to.display(),
                            MAX_RETRIES
                        ),
                        &[
                            "This usually means that the application is still running in another terminal. Please exit any running instances (including shells launched by it) before trying again.",
                            "Make sure that you have write permissions on the application file, or try running the application with elevated permissions.",
                        ],
                    ));
                }
                Err(_) => {
                    debug!(
                        "Failed to copy '{}' -> '{}' (attempt {}); retrying.",
                        from.display(),
                        to.display(),
                        attempt + 1
                    );
                    attempt += 1;
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
    }

    fn get_temp_app_path(&self, target_application: &Path, release: &Release) -> PathBuf {
        let stem = target_application
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("update");

        let ext = target_application
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    ".exe".to_string()
                } else {
                    String::new()
                }
            });

        std::env::temp_dir().join(format!("{stem}-{}{ext}", release.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_release() -> Release {
        Release {
            id: "v1.0.0".to_string(),
            changelog: String::new(),
            version: "1.0.0".parse().unwrap(),
            prerelease: false,
            variant: None,
        }
    }

    #[tokio::test]
    async fn test_delete_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.txt");
        tokio::fs::write(&path, "test").await.unwrap();

        let fs = DefaultFileSystem {};
        fs.delete_file(&path).await.unwrap();

        assert!(!path.exists());
    }

    #[tokio::test]
    async fn test_delete_missing_file_is_ok() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("does-not-exist.txt");

        let fs = DefaultFileSystem {};
        fs.delete_file(&path).await.unwrap();
    }

    #[tokio::test]
    async fn test_copy_file() {
        let temp = tempfile::tempdir().unwrap();
        let from = temp.path().join("from.txt");
        let to = temp.path().join("to.txt");
        tokio::fs::write(&from, "test").await.unwrap();

        let fs = DefaultFileSystem {};
        fs.copy_file(&from, &to).await.unwrap();

        assert!(to.exists());
        assert_eq!(tokio::fs::read_to_string(&to).await.unwrap(), "test");
    }

    #[test]
    fn test_get_temp_app_path() {
        let fs = DefaultFileSystem {};
        let release = test_release();

        let with_ext = fs.get_temp_app_path(Path::new("/usr/bin/myapp.exe"), &release);
        assert_eq!(with_ext.file_name().unwrap(), "myapp-v1.0.0.exe");
        assert_eq!(with_ext.parent().unwrap(), std::env::temp_dir());

        let without_ext = fs.get_temp_app_path(Path::new("/usr/bin/myapp"), &release);
        #[cfg(windows)]
        assert_eq!(without_ext.file_name().unwrap(), "myapp-v1.0.0.exe");
        #[cfg(not(windows))]
        assert_eq!(without_ext.file_name().unwrap(), "myapp-v1.0.0");
    }
}
