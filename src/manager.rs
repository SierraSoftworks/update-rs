#[cfg(not(feature = "tracing"))]
use log::{debug, info};
#[cfg(feature = "tracing")]
use tracing::{debug, info};

use crate::{Error, GitHubSource, Release, Source, UpdatePhase, UpdateState, cmd, fs};
use human_errors::{OptionExt, ResultExt};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Drives the three-phase, in-place self-update of an application binary.
///
/// An `UpdateManager` lists the releases offered by its [`Source`], downloads
/// the asset the source selected for this platform, and then walks through the
/// `prepare → replace → cleanup` phases by relaunching the application between
/// each phase (see the [crate-level docs](crate) and [`RESUME_FLAG`](crate::RESUME_FLAG)).
///
/// Construct one with [`UpdateManager::new`] (which targets the currently
/// running executable) and customise it with
/// [`with_target_application`](Self::with_target_application) if needed. Which
/// release asset is downloaded is configured on the [`Source`] — see
/// [`GitHubSource`].
pub struct UpdateManager<S = GitHubSource>
where
    S: Source,
{
    /// The application binary which will be replaced by the update. Defaults to
    /// the currently running executable.
    pub target_application: PathBuf,

    /// The source releases are listed and downloaded from.
    pub source: S,

    launcher: Box<dyn cmd::Launcher + Send + Sync>,
    filesystem: Box<dyn fs::FileSystem + Send + Sync>,
}

impl<S> UpdateManager<S>
where
    S: Source,
{
    /// Create a manager which will update the currently running executable
    /// using the provided release `source`.
    pub fn new(source: S) -> Self {
        Self {
            target_application: std::env::current_exe().unwrap_or_default(),
            source,
            launcher: cmd::default(),
            filesystem: fs::default(),
        }
    }

    /// Override the application binary which will be updated (defaults to the
    /// currently running executable).
    pub fn with_target_application(mut self, target_application: PathBuf) -> Self {
        self.target_application = target_application;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_mock_launcher<M: FnMut(&mut cmd::MockLauncher)>(
        mut self,
        mut setup: M,
    ) -> Self {
        let mut mock = cmd::MockLauncher::new();
        setup(&mut mock);
        self.launcher = Box::new(mock);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_mock_fs<M: FnMut(&mut fs::MockFileSystem)>(mut self, mut setup: M) -> Self {
        let mut mock = fs::MockFileSystem::new();
        setup(&mut mock);
        self.filesystem = Box::new(mock);
        self
    }

    /// List the releases available from the configured [`Source`].
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self)))]
    pub async fn get_releases(&self) -> Result<Vec<Release>, Error> {
        self.source.get_releases().await
    }

    /// Begin updating the [`target_application`](Self::target_application) to
    /// the provided release.
    ///
    /// This downloads the new binary, then launches it to continue the update
    /// in a separate process. Returns `Ok(true)` if an update was started, in
    /// which case the caller should exit promptly so the relaunched process can
    /// replace the running binary.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self, release), fields(release = %release.id, version = %release.version))
    )]
    pub async fn update(&self, release: &Release) -> Result<bool, Error> {
        let state = UpdateState {
            target_application: Some(self.target_application.clone()),
            temporary_application: Some(
                self.filesystem
                    .get_temp_app_path(&self.target_application, release),
            ),
            trace_context: None,
            phase: UpdatePhase::Prepare,
        };

        let app = state.temporary_application.clone().ok_or_system_err(
            "A temporary application path was not provided and the update cannot proceed (prepare -> replace phase).",
            &["Please report this issue to the application's maintainers, or try updating manually by downloading the latest release yourself."],
        )?;

        let variant = release.get_variant().ok_or_user_err(
            format!(
                "No release asset for {} matched the configured artifact pattern.",
                release.id
            ),
            &["Check that your project publishes a release asset matching the pattern you configured for this platform."],
        )?;

        {
            info!(
                "Checking whether the application binary ({}) is writable by the current user.",
                self.target_application.display()
            );
            let metadata = tokio::fs::metadata(&self.target_application).await.wrap_user_err(
                format!(
                    "Failed to read the current file state of the application binary ({}).",
                    self.target_application.display()
                ),
                &[
                    "Please ensure that the application binary exists and that this tool has permission to read and write to it.",
                    "Try running the update command again with elevated permissions.",
                ],
            )?;

            if metadata.permissions().readonly() {
                return Err(human_errors::user(
                    "The application binary is read-only, so it cannot be replaced by the update.",
                    &{
                        #[cfg(windows)]
                        {
                            [
                                "Make sure the binary is writable, or try running this command in an administrative console (Win+X, A).",
                            ]
                        }

                        #[cfg(unix)]
                        {
                            [
                                "Make sure the binary is writable, or try running this command as root (e.g. with `sudo`).",
                            ]
                        }
                    },
                ));
            }
        }

        {
            info!(
                "Downloading release binary for {} to a temporary location ({}).",
                release.version,
                app.display()
            );
            let mut app_file = std::fs::File::create(&app).wrap_user_err(
                format!(
                    "Could not create the new application file '{}' due to an OS-level error.",
                    app.display()
                ),
                &["Check that this tool has permission to create and write to this file and that the parent directory exists."],
            )?;
            if let Err(e) = self
                .source
                .get_binary(release, variant, &mut app_file)
                .await
            {
                // Don't leave a partial or failed-verification download behind.
                drop(app_file);
                let _ = std::fs::remove_file(&app);
                return Err(e);
            }

            debug!("Preparing the downloaded application file for execution.");
            self.prepare_app_file(&app)?;
        }

        self.resume(&state).await
    }

    /// Resume an update from a previously serialized [`UpdateState`].
    ///
    /// A consuming application calls this (usually via
    /// [`resume_from_arg`](Self::resume_from_arg)) when it is relaunched with
    /// the [`RESUME_FLAG`](crate::RESUME_FLAG). Returns `Ok(true)` when a phase
    /// was processed and the process should exit.
    pub async fn resume(&self, state: &UpdateState) -> Result<bool, Error> {
        #[cfg(feature = "tracing")]
        {
            use tracing::Instrument;

            let span = tracing::info_span!("resume", phase = %state.phase);
            // Re-parent this span onto the trace carried from the phase that
            // relaunched us *before* it is entered, so all the work in this
            // process continues that distributed trace. A no-op without the
            // `opentelemetry` feature or a carried context.
            state.adopt_trace_context(&span);
            self.dispatch(state).instrument(span).await
        }

        #[cfg(not(feature = "tracing"))]
        {
            self.dispatch(state).await
        }
    }

    async fn dispatch(&self, state: &UpdateState) -> Result<bool, Error> {
        match state.phase {
            UpdatePhase::NoUpdate => Ok(false),
            UpdatePhase::Prepare => self.prepare(state).await,
            UpdatePhase::Replace => self.replace(state).await,
            UpdatePhase::Cleanup => self.cleanup(state).await,
        }
    }

    /// Deserialize an [`UpdateState`] from the JSON argument that follows the
    /// [`RESUME_FLAG`](crate::RESUME_FLAG) on the command line, then
    /// [`resume`](Self::resume) the update from it.
    pub async fn resume_from_arg(&self, state_json: &str) -> Result<bool, Error> {
        let state: UpdateState = serde_json::from_str(state_json).wrap_system_err(
            "Could not deserialize the update state which was passed on the command line.",
            &["Please report this issue to the application's maintainers and use the manual update process until it is resolved."],
        )?;

        info!("Resuming update in the '{}' phase.", state.phase);
        self.resume(&state).await
    }

    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, state)))]
    async fn prepare(&self, state: &UpdateState) -> Result<bool, Error> {
        let mut next_state = state.for_phase(UpdatePhase::Replace);
        // Hand the active trace context to the next process so the update's
        // phases stitch together into a single distributed trace.
        next_state.capture_trace_context();
        let update_source = state.temporary_application.clone().ok_or_system_err(
            "Could not launch the new application version to continue the update process (prepare -> replace phase).",
            &["Please report this issue to the application's maintainers, or try updating manually by downloading the latest release yourself."],
        )?;

        info!(
            "Launching the temporary release binary to perform the 'replace' phase of the update."
        );
        self.launch(&update_source, &next_state)?;

        Ok(true)
    }

    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, state)))]
    async fn replace(&self, state: &UpdateState) -> Result<bool, Error> {
        let update_source = state.temporary_application.clone().ok_or_system_err(
            "Could not locate the temporary update files needed to complete the update process (replace phase).",
            &["Please report this issue to the application's maintainers, or try updating manually by downloading the latest release yourself."],
        )?;
        let update_target = state.target_application.clone().ok_or_system_err(
            "Could not locate the application which was meant to be updated (replace phase).",
            &["Please report this issue to the application's maintainers, or try updating manually by downloading the latest release yourself."],
        )?;

        info!("Removing the original application binary to avoid conflicts with open handles.");
        self.filesystem.delete_file(&update_target).await?;

        info!("Replacing the original application binary with the temporary release binary.");
        self.filesystem
            .copy_file(&update_source, &update_target)
            .await?;

        info!("Launching the updated application to perform the 'cleanup' phase of the update.");
        let mut next_state = state.for_phase(UpdatePhase::Cleanup);
        // Carry the active trace context forward into the final phase too.
        next_state.capture_trace_context();
        self.launch(&update_target, &next_state)?;

        Ok(true)
    }

    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, state)))]
    async fn cleanup(&self, state: &UpdateState) -> Result<bool, Error> {
        let update_source = state.temporary_application.clone().ok_or_system_err(
            "Could not locate the temporary update files needed to complete the update process (cleanup phase).",
            &["Please report this issue to the application's maintainers, or try updating manually by downloading the latest release yourself."],
        )?;

        info!("Removing the temporary update application binary.");
        self.filesystem.delete_file(&update_source).await?;

        Ok(true)
    }

    #[cfg(unix)]
    fn prepare_app_file(&self, file: &Path) -> Result<(), Error> {
        let mut perms = std::fs::metadata(file)
            .wrap_user_err(
                format!(
                    "Could not read the permissions of '{}' due to an OS-level error.",
                    file.display()
                ),
                &["Check that this tool has permission to read this file and that the parent directory exists."],
            )?
            .permissions();

        debug!("Setting executable permissions (0o755) on the downloaded application binary.");
        perms.set_mode(0o755);
        std::fs::set_permissions(file, perms).wrap_user_err(
            format!(
                "Could not set executable permissions on '{}' due to an OS-level error.",
                file.display()
            ),
            &["Check that this tool has permission to modify this file and that the parent directory exists."],
        )?;

        Ok(())
    }

    #[cfg(not(unix))]
    fn prepare_app_file(&self, _file: &Path) -> Result<(), Error> {
        Ok(())
    }

    fn launch(&self, app_path: &Path, state: &UpdateState) -> Result<(), Error> {
        self.launcher.launch(app_path, state)
    }
}

impl<S> Default for UpdateManager<S>
where
    S: Source,
{
    fn default() -> Self {
        Self::new(S::default())
    }
}

impl<S> std::fmt::Debug for UpdateManager<S>
where
    S: Source,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", &self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path, path_regex};
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
                { "name": "update-darwin-arm64" }
            ]
        }
    ]"#;

    #[tokio::test]
    async fn test_update() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/sierrasoftworks/update-rs/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RELEASES_JSON))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(
                r"^/sierrasoftworks/update-rs/releases/download/",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string("testdata"))
            .mount(&server)
            .await;

        let temp = tempdir().unwrap();
        let app_path = temp.path().join("app");
        let temp_app_path = temp.path().join("app-temp");
        std::fs::write(&app_path, "Pre-Update").unwrap();

        // A fixed pattern so the test is independent of the host platform.
        let source = GitHubSource::new("sierrasoftworks/update-rs", "update-linux-amd64")
            .with_github_endpoints(&server.uri(), &server.uri())
            .with_release_tag_prefix("v");

        let manager = UpdateManager::new(source)
            .with_target_application(app_path.clone())
            .with_mock_launcher(|mock| {
                let temp_app_path = temp_app_path.clone();
                mock.expect_launch()
                    .once()
                    .withf(move |p, s| p == temp_app_path && s.phase == UpdatePhase::Replace)
                    .returning(|_, _| Ok(()));
            })
            .with_mock_fs(|mock| {
                mock.expect_get_temp_app_path()
                    .once()
                    .return_const(temp_app_path.clone());
            });

        let releases = manager
            .get_releases()
            .await
            .expect("we should receive a release entry");
        let latest =
            Release::get_latest(releases.iter()).expect("we should receive a latest release entry");

        let has_update = manager
            .update(latest)
            .await
            .expect("the update operation should succeed");

        assert!(has_update, "the update should be applied");
    }

    #[tokio::test]
    async fn test_update_resume() {
        let temp = tempdir().unwrap();
        let app_path = temp.path().join("app");
        let temp_app_path = temp.path().join("app-temp");
        std::fs::write(&app_path, "original").unwrap();
        std::fs::write(&temp_app_path, "new").unwrap();

        let manager = UpdateManager::<GitHubSource>::default()
            .with_target_application(app_path.clone())
            .with_mock_launcher(|mock| {
                let app_path = app_path.clone();
                mock.expect_launch()
                    .once()
                    .withf(move |p, s| p == app_path && s.phase == UpdatePhase::Cleanup)
                    .returning(|_, _| Ok(()));
            })
            .with_mock_fs(|mock| {
                let app_path = app_path.clone();
                let app_path_for_copy = app_path.clone();
                let temp_app_path = temp_app_path.clone();
                mock.expect_get_temp_app_path().never();
                mock.expect_delete_file()
                    .once()
                    .withf(move |p| p == app_path)
                    .returning(|_| Ok(()));
                mock.expect_copy_file()
                    .once()
                    .withf(move |src, dst| src == temp_app_path && dst == app_path_for_copy)
                    .returning(|_, _| Ok(()));
            });

        let state = UpdateState {
            phase: UpdatePhase::Replace,
            target_application: Some(app_path.clone()),
            temporary_application: Some(temp_app_path.clone()),
            trace_context: None,
        };

        let has_update = manager
            .resume(&state)
            .await
            .expect("the update operation should succeed");

        assert!(has_update, "the update should be applied");
    }

    #[tokio::test]
    async fn test_update_cleanup() {
        let temp = tempdir().unwrap();
        let app_path = temp.path().join("app");
        let temp_app_path = temp.path().join("app-temp");
        std::fs::write(&app_path, "original").unwrap();
        std::fs::write(&temp_app_path, "new").unwrap();

        let manager = UpdateManager::<GitHubSource>::default()
            .with_target_application(app_path.clone())
            .with_mock_launcher(|mock| {
                mock.expect_spawn().never();
            })
            .with_mock_fs(|mock| {
                let temp_app_path = temp_app_path.clone();
                mock.expect_get_temp_app_path().never();
                mock.expect_delete_file()
                    .once()
                    .withf(move |p| p == temp_app_path)
                    .returning(|p| {
                        std::fs::remove_file(p).expect("we should be able to delete the path");
                        Ok(())
                    });
            });

        let state = UpdateState {
            phase: UpdatePhase::Cleanup,
            target_application: Some(app_path.clone()),
            temporary_application: Some(temp_app_path.clone()),
            trace_context: None,
        };

        let has_update = manager
            .resume(&state)
            .await
            .expect("the update operation should succeed");

        assert!(has_update, "the update should be applied");
        assert!(app_path.exists(), "the app should still be present");
        assert!(
            !temp_app_path.exists(),
            "the temp app should have been removed"
        );
    }
}
