#[cfg(not(feature = "tracing"))]
use log::debug;
#[cfg(feature = "tracing")]
use tracing::debug;

use crate::{Error, RESUME_FLAG, UpdateState};
use human_errors::ResultExt;
use std::ffi::OsString;
use std::{path::Path, process::Command};

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

/// Extra command-line arguments and environment variables a consumer wants every
/// relaunched update process to receive, on top of the
/// [`RESUME_FLAG`](crate::RESUME_FLAG) and serialized state the library always
/// provides.
///
/// Populated through the `with_relaunch_*` builder methods on
/// [`UpdateManager`](crate::UpdateManager) and handed to
/// [`Launcher::launch`]. Lets an application thread its own context through the
/// update — a `--trace-context` argument, an `APP_UPDATING=1` environment
/// variable, a channel/verbosity flag, and so on.
#[derive(Debug, Default, Clone)]
pub(crate) struct Relaunch {
    pub(crate) args: Vec<OsString>,
    pub(crate) envs: Vec<(OsString, OsString)>,
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
    /// Relaunch `app_path` to continue the update with the provided state,
    /// appending the consumer's custom arguments and environment variables from
    /// `customization`.
    fn launch(
        &self,
        app_path: &Path,
        state: &UpdateState,
        customization: &Relaunch,
    ) -> Result<(), Error> {
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
        // The library's own resume flag comes first, then whatever the
        // application asked to be threaded through to the next process.
        cmd.arg(RESUME_FLAG).arg(&state_json);
        cmd.args(&customization.args);
        cmd.envs(customization.envs.iter().map(|(k, v)| (k, v)));

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UpdatePhase;
    use std::sync::Mutex;

    /// A [`Launcher`] that captures the prepared [`Command`] instead of spawning
    /// it, so we can assert what the default `launch` would run.
    struct CapturingLauncher {
        captured: Mutex<Option<Command>>,
    }

    impl Launcher for CapturingLauncher {
        fn spawn(&self, cmd: Command) -> Result<(), Error> {
            *self.captured.lock().unwrap() = Some(cmd);
            Ok(())
        }
    }

    #[test]
    fn launch_threads_custom_args_and_env_after_the_resume_flag() {
        let launcher = CapturingLauncher {
            captured: Mutex::new(None),
        };
        let state = UpdateState {
            phase: UpdatePhase::Replace,
            ..Default::default()
        };
        let customization = Relaunch {
            args: vec!["--trace-context".into(), "ctx-json".into()],
            envs: vec![("APP_UPDATING".into(), "1".into())],
        };

        launcher
            .launch(Path::new("app"), &state, &customization)
            .expect("the capturing launcher never fails");

        let cmd = launcher.captured.lock().unwrap().take().unwrap();

        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        // The library's resume flag and serialized state always come first, then
        // the consumer's extra arguments, in order.
        assert_eq!(args[0], RESUME_FLAG);
        assert_eq!(&args[2..], &["--trace-context", "ctx-json"]);

        let env: Vec<(String, Option<String>)> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert!(
            env.contains(&("APP_UPDATING".to_string(), Some("1".to_string()))),
            "custom environment variable should be set on the relaunch: {env:?}"
        );
    }

    #[test]
    fn launch_without_customization_only_passes_the_resume_flag() {
        let launcher = CapturingLauncher {
            captured: Mutex::new(None),
        };
        let state = UpdateState {
            phase: UpdatePhase::Cleanup,
            ..Default::default()
        };

        launcher
            .launch(Path::new("app"), &state, &Relaunch::default())
            .expect("the capturing launcher never fails");

        let cmd = launcher.captured.lock().unwrap().take().unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args.len(), 2, "only the resume flag and state: {args:?}");
        assert_eq!(args[0], RESUME_FLAG);
        assert_eq!(cmd.get_envs().count(), 0, "no custom environment variables");
    }
}
