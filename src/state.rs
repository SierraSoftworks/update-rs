use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::PathBuf;

/// The phase of the three-phase update process which an [`UpdateState`] is in.
///
/// The update is driven forward by relaunching the application between phases,
/// passing a serialized [`UpdateState`] each time (see
/// [`RESUME_FLAG`](crate::RESUME_FLAG)). Each phase runs from a *different*
/// binary so that the running executable is never asked to overwrite itself.
#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
pub enum UpdatePhase {
    /// No update is in progress (the default, used when resuming with no state).
    #[default]
    #[serde(rename = "no-update")]
    NoUpdate,
    /// The original application has downloaded the new binary and is about to
    /// launch it to perform the `replace` phase.
    #[serde(rename = "prepare")]
    Prepare,
    /// The freshly downloaded binary overwrites the original application and
    /// launches it to perform the `cleanup` phase.
    #[serde(rename = "replace")]
    Replace,
    /// The updated original application removes the temporary downloaded binary.
    #[serde(rename = "cleanup")]
    Cleanup,
}

impl Display for UpdatePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdatePhase::NoUpdate => write!(f, "no-update"),
            UpdatePhase::Prepare => write!(f, "prepare"),
            UpdatePhase::Replace => write!(f, "replace"),
            UpdatePhase::Cleanup => write!(f, "cleanup"),
        }
    }
}

/// The serializable state which is threaded through the three phases of an
/// update by relaunching the application with the
/// [`RESUME_FLAG`](crate::RESUME_FLAG) followed by this value as JSON.
#[derive(Debug, Default, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct UpdateState {
    /// The path to the application binary which is being updated.
    #[serde(rename = "app", default, skip_serializing_if = "Option::is_none")]
    pub target_application: Option<PathBuf>,

    /// The path to the temporary binary which the new release was downloaded to.
    #[serde(rename = "update", default, skip_serializing_if = "Option::is_none")]
    pub temporary_application: Option<PathBuf>,

    /// The phase of the update process which this state represents.
    pub phase: UpdatePhase,
}

impl UpdateState {
    /// Produce a copy of this state advanced to the provided [`UpdatePhase`],
    /// preserving the application paths.
    pub fn for_phase(&self, phase: UpdatePhase) -> Self {
        UpdateState {
            target_application: self.target_application.clone(),
            temporary_application: self.temporary_application.clone(),
            phase,
        }
    }
}

impl Display for UpdateState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.phase)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize() {
        assert_eq!(
            serde_json::to_string(&UpdateState {
                target_application: Some(PathBuf::from("/bin/app")),
                temporary_application: Some(PathBuf::from("/tmp/app-update")),
                phase: UpdatePhase::Replace
            })
            .unwrap(),
            r#"{"app":"/bin/app","update":"/tmp/app-update","phase":"replace"}"#
        );

        assert_eq!(
            serde_json::to_string(&UpdateState {
                target_application: None,
                temporary_application: Some(PathBuf::from("/tmp/app-update")),
                phase: UpdatePhase::Cleanup
            })
            .unwrap(),
            r#"{"update":"/tmp/app-update","phase":"cleanup"}"#
        );
    }

    #[test]
    fn test_deserialize() {
        let update = UpdateState {
            target_application: None,
            temporary_application: Some(PathBuf::from("/tmp/app-update")),
            phase: UpdatePhase::Cleanup,
        };

        let deserialized: UpdateState =
            serde_json::from_str(r#"{"update":"/tmp/app-update","phase":"cleanup"}"#).unwrap();
        assert_eq!(deserialized, update);
    }

    #[test]
    fn test_to_string() {
        assert_eq!(UpdatePhase::Prepare.to_string(), "prepare");
        assert_eq!(UpdatePhase::Replace.to_string(), "replace");
        assert_eq!(UpdatePhase::Cleanup.to_string(), "cleanup");
        assert_eq!(UpdatePhase::NoUpdate.to_string(), "no-update");
    }

    #[test]
    fn test_for_phase() {
        let update = UpdateState {
            target_application: Some(PathBuf::from("/bin/app")),
            temporary_application: Some(PathBuf::from("/tmp/app-update")),
            phase: UpdatePhase::Replace,
        };

        let new_update = update.for_phase(UpdatePhase::Cleanup);
        assert_eq!(new_update.target_application, update.target_application);
        assert_eq!(
            new_update.temporary_application,
            update.temporary_application
        );
        assert_eq!(
            update.phase,
            UpdatePhase::Replace,
            "the old update entry should not be modified"
        );
        assert_eq!(
            new_update.phase,
            UpdatePhase::Cleanup,
            "the new update entry should have the correct phase"
        );
    }
}
