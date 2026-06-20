use crate::{Error, RESUME_FLAG, UpdateState};
use human_errors::ResultExt;
use std::{path::Path, process::Command};
use tracing::debug;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(test)]
use mockall::automock;

#[cfg(windows)]
mod windows {
    /// Detach the relaunched process from the current console so it survives the
    /// parent exiting, and give it its own process group.
    pub const DETACHED_PROCESS: u32 = 0x0000_0008;
    pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
}

pub(crate) fn default() -> Box<dyn Launcher + Send + Sync> {
    Box::new(DefaultLauncher {})
}

/// Launches the application binary to drive the next phase of an update.
///
/// The default [`launch`](Launcher::launch) implementation serializes the
/// [`UpdateState`] and invokes the binary with
/// [`RESUME_FLAG`](crate::RESUME_FLAG) followed by the JSON state, so that a
/// consuming `main()` can detect it and resume the update. It is abstracted
/// behind a trait so the manager's state machine can be unit-tested without
/// actually spawning processes.
#[cfg_attr(test, automock)]
pub trait Launcher {
    /// Relaunch `app_path` to continue the update with the provided state.
    fn launch(&self, app_path: &Path, state: &UpdateState) -> Result<(), Error> {
        let state_json = serde_json::to_string(state).wrap_system_err(
            "Failed to serialize the update state into a JSON payload.",
            &["Please report this issue to the application's maintainers."],
        )?;

        debug!(
            "Launching '{}' to perform the '{}' phase of the update.",
            app_path.display(),
            state.phase
        );

        let mut cmd = Command::new(app_path);
        cmd.arg(RESUME_FLAG).arg(&state_json);

        #[cfg(windows)]
        cmd.creation_flags(windows::DETACHED_PROCESS | windows::CREATE_NEW_PROCESS_GROUP);

        self.spawn(cmd).wrap_system_err(
            format!(
                "Could not launch the new application version to continue the update process (-> {} phase).",
                state.phase
            ),
            &["Please report this issue to the application's maintainers, or try updating manually by downloading the latest release yourself."],
        )
    }

    /// Spawn the prepared [`Command`]. This is the single seam the default
    /// [`launch`](Launcher::launch) relies on, so tests can mock it.
    fn spawn(&self, cmd: Command) -> Result<(), Error>;
}

struct DefaultLauncher {}

impl Launcher for DefaultLauncher {
    fn spawn(&self, mut cmd: Command) -> Result<(), Error> {
        cmd.spawn().wrap_user_err(
            "Failed to launch the application to complete the update process.",
            &[
                "Try re-running the update.",
                "Download the latest release and install it manually if the problem continues.",
            ],
        )?;

        Ok(())
    }
}
