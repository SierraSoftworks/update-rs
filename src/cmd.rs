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
    Box::new(DefaultLauncher)
}

/// Extra command-line arguments and environment variables a consumer wants every
/// relaunched update process to receive, on top of the arguments the
/// [`Launcher`] adds to carry the update state.
///
/// Populated through the `with_relaunch_*` builder methods on
/// [`UpdateManager`](crate::UpdateManager) and handed to [`Launcher::launch`].
/// Lets an application thread its own context through the update — a
/// `--trace-context` argument, an `APP_UPDATING=1` environment variable, a
/// channel/verbosity flag, and so on.
#[derive(Debug, Default, Clone)]
pub struct Relaunch {
    pub(crate) args: Vec<OsString>,
    pub(crate) envs: Vec<(OsString, OsString)>,
}

impl Relaunch {
    /// The extra arguments to append to each relaunch command.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// The extra environment variables (key, value) to set on each relaunched
    /// process.
    pub fn envs(&self) -> &[(OsString, OsString)] {
        &self.envs
    }
}

/// Launches the application binary to drive the next phase of an update.
///
/// Every method has a default implementation, so the simplest custom launcher is
/// an empty `impl Launcher for MyLauncher {}` (equivalent to [`DefaultLauncher`]).
/// Customise as much or as little as you need:
///
/// - override [`resume_args`](Launcher::resume_args) to change *how* the update
///   state is handed to the relaunched process (e.g. via a sub-command rather
///   than the default [`RESUME_FLAG`](crate::RESUME_FLAG));
/// - override [`launch`](Launcher::launch) for complete control over the relaunch
///   command;
/// - override [`spawn`](Launcher::spawn) to change how the child process is
///   started.
///
/// Install a custom launcher with
/// [`UpdateManager::with_launcher`](crate::UpdateManager::with_launcher).
#[cfg_attr(test, automock)]
pub trait Launcher {
    /// The arguments that carry the serialized resume `state_json` to the
    /// relaunched process.
    ///
    /// The default passes the library's [`RESUME_FLAG`](crate::RESUME_FLAG)
    /// followed by the JSON, which a consuming `main()` detects (before any other
    /// argument parsing) and forwards to
    /// [`resume_from_arg`](crate::UpdateManager::resume_from_arg). Override it to
    /// use a different convention — for example handing the state to a
    /// sub-command:
    ///
    /// ```
    /// use std::ffi::OsString;
    /// use update_rs::Launcher;
    ///
    /// struct SubcommandLauncher;
    /// impl Launcher for SubcommandLauncher {
    ///     fn resume_args(&self, state_json: &str) -> Vec<OsString> {
    ///         vec!["update".into(), "--state".into(), state_json.into()]
    ///     }
    /// }
    /// ```
    fn resume_args(&self, state_json: &str) -> Vec<OsString> {
        vec![RESUME_FLAG.into(), state_json.into()]
    }

    /// Build and spawn the command that relaunches `app_path` to continue the
    /// update with `state`, appending the consumer's custom arguments and
    /// environment variables from `customization`.
    ///
    /// The default builds `app_path <`[`resume_args`](Launcher::resume_args)`>
    /// <customization args>` with the customization's environment variables and
    /// the platform [`detach`](Launcher::detach) flags, then
    /// [`spawn`](Launcher::spawn)s it. Override this for complete control over the
    /// relaunch command (reusing [`detach`](Launcher::detach) and
    /// [`spawn`](Launcher::spawn) as needed).
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
        // The arguments carrying the update state come first, then whatever the
        // application asked to be threaded through to the next process.
        cmd.args(self.resume_args(&state_json));
        cmd.args(customization.args());
        cmd.envs(customization.envs().iter().map(|(k, v)| (k, v)));
        self.detach(&mut cmd);

        self.spawn(cmd).wrap_system_err(
            format!(
                "Could not launch the new application version to continue the update process (-> {} phase).",
                state.phase
            ),
            &["Please report this issue to the application's maintainers, or try updating manually by downloading the latest release yourself."],
        )
    }

    /// Apply the platform-specific flags that detach the relaunched process from
    /// the current console so it survives the parent exiting (a detached process
    /// in its own group on Windows). A no-op on non-Windows platforms. Exposed so
    /// custom [`launch`](Launcher::launch) implementations can reuse it.
    fn detach(&self, cmd: &mut Command) {
        #[cfg(windows)]
        cmd.creation_flags(windows::DETACHED_PROCESS | windows::CREATE_NEW_PROCESS_GROUP);
        #[cfg(not(windows))]
        let _ = cmd;
    }

    /// Spawn the prepared [`Command`]. The default starts the child process and
    /// returns immediately. This is the single seam the default
    /// [`launch`](Launcher::launch) relies on, so tests can mock it.
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

/// The default [`Launcher`]: relaunches with the library's
/// [`RESUME_FLAG`](crate::RESUME_FLAG) and spawns a detached child process. Used
/// unless [`UpdateManager::with_launcher`](crate::UpdateManager::with_launcher)
/// installs a different one.
#[derive(Debug, Default)]
pub struct DefaultLauncher;

impl Launcher for DefaultLauncher {}

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

    impl CapturingLauncher {
        fn new() -> Self {
            Self {
                captured: Mutex::new(None),
            }
        }

        fn args(&self) -> Vec<String> {
            self.captured
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        }
    }

    impl Launcher for CapturingLauncher {
        fn spawn(&self, cmd: Command) -> Result<(), Error> {
            *self.captured.lock().unwrap() = Some(cmd);
            Ok(())
        }
    }

    #[test]
    fn launch_threads_custom_args_and_env_after_the_resume_flag() {
        let launcher = CapturingLauncher::new();
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

        let args = launcher.args();
        // The library's resume flag and serialized state always come first, then
        // the consumer's extra arguments, in order.
        assert_eq!(args[0], RESUME_FLAG);
        assert_eq!(&args[2..], &["--trace-context", "ctx-json"]);

        let cmd = launcher.captured.lock().unwrap().take().unwrap();
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
        let launcher = CapturingLauncher::new();
        let state = UpdateState {
            phase: UpdatePhase::Cleanup,
            ..Default::default()
        };

        launcher
            .launch(Path::new("app"), &state, &Relaunch::default())
            .expect("the capturing launcher never fails");

        let args = launcher.args();
        assert_eq!(args.len(), 2, "only the resume flag and state: {args:?}");
        assert_eq!(args[0], RESUME_FLAG);
        assert_eq!(
            launcher
                .captured
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .get_envs()
                .count(),
            0,
            "no custom environment variables"
        );
    }

    /// A launcher that hands the update state to an `update --state <json>`
    /// sub-command instead of the default resume flag (Git-Tool's convention).
    struct SubcommandLauncher {
        inner: CapturingLauncher,
    }

    impl Launcher for SubcommandLauncher {
        fn resume_args(&self, state_json: &str) -> Vec<OsString> {
            vec!["update".into(), "--state".into(), state_json.into()]
        }

        fn spawn(&self, cmd: Command) -> Result<(), Error> {
            self.inner.spawn(cmd)
        }
    }

    #[test]
    fn launch_honours_a_custom_resume_args_convention() {
        let launcher = SubcommandLauncher {
            inner: CapturingLauncher::new(),
        };
        let state = UpdateState {
            phase: UpdatePhase::Replace,
            ..Default::default()
        };

        launcher
            .launch(Path::new("app"), &state, &Relaunch::default())
            .expect("the capturing launcher never fails");

        let args = launcher.inner.args();
        assert_eq!(&args[..2], &["update", "--state"]);
        assert!(
            args[2].contains("\"phase\":\"replace\""),
            "the serialized state should follow the sub-command: {args:?}"
        );
        assert!(
            !args.iter().any(|a| a == RESUME_FLAG),
            "the default resume flag should have been replaced: {args:?}"
        );
    }
}
