use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

    /// A trace-context propagation carrier (e.g. the W3C `traceparent` /
    /// `tracestate` headers) captured from the phase that relaunched the
    /// application. With the `opentelemetry` feature it lets the phases of an
    /// update — which each run in a separate process — continue a single
    /// distributed trace. The field is always part of the wire format (so states
    /// stay compatible across feature configurations) but is only populated and
    /// consumed when that feature is enabled.
    #[serde(rename = "trace", default, skip_serializing_if = "Option::is_none")]
    pub trace_context: Option<HashMap<String, String>>,

    /// The phase of the update process which this state represents.
    pub phase: UpdatePhase,
}

impl UpdateState {
    /// Produce a copy of this state advanced to the provided [`UpdatePhase`],
    /// preserving the application paths and any captured trace context.
    pub fn for_phase(&self, phase: UpdatePhase) -> Self {
        UpdateState {
            target_application: self.target_application.clone(),
            temporary_application: self.temporary_application.clone(),
            trace_context: self.trace_context.clone(),
            phase,
        }
    }

    /// Capture the active OpenTelemetry trace context into this state so the next
    /// phase — launched as a separate process — can continue the same
    /// distributed trace.
    ///
    /// Only the *global* propagator
    /// (`opentelemetry::global::get_text_map_propagator`) is consulted, so this
    /// honours whatever propagation the host application configured. It is a
    /// no-op when the `opentelemetry` feature is disabled, or when no propagator
    /// is installed (in which case nothing is captured and the state stays
    /// untouched).
    #[cfg(feature = "opentelemetry")]
    pub(crate) fn capture_trace_context(&mut self) {
        use tracing_opentelemetry::OpenTelemetrySpanExt;

        let context = tracing::Span::current().context();
        let mut carrier = HashMap::new();
        opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&context, &mut carrier);
        });

        // Only carry a context when the propagator actually produced one, so a
        // host without OpenTelemetry configured doesn't bloat the state payload.
        if !carrier.is_empty() {
            self.trace_context = Some(carrier);
        }
    }

    /// No-op stand-in used when the `opentelemetry` feature is disabled.
    #[cfg(not(feature = "opentelemetry"))]
    pub(crate) fn capture_trace_context(&mut self) {}

    /// Adopt the trace context carried in this state (if any) as the parent of
    /// `span`, continuing the distributed trace started by the phase that
    /// relaunched us.
    ///
    /// This **must** be called before `span` is entered: a span's trace identity
    /// is fixed once it becomes active, so re-parenting afterwards would have no
    /// effect. It is a no-op when the `opentelemetry` feature is disabled or no
    /// context was carried.
    #[cfg(feature = "tracing")]
    pub(crate) fn adopt_trace_context(&self, span: &tracing::Span) {
        #[cfg(feature = "opentelemetry")]
        {
            use tracing_opentelemetry::OpenTelemetrySpanExt;

            if let Some(carrier) = &self.trace_context {
                let parent = opentelemetry::global::get_text_map_propagator(|propagator| {
                    propagator.extract(carrier)
                });
                // Best-effort: if no OpenTelemetry layer is installed there is
                // simply no span to re-parent, which is fine.
                let _ = span.set_parent(parent);
            }
        }

        // Without `opentelemetry` there is no context to adopt.
        #[cfg(not(feature = "opentelemetry"))]
        let _ = span;
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
                trace_context: None,
                phase: UpdatePhase::Replace
            })
            .unwrap(),
            r#"{"app":"/bin/app","update":"/tmp/app-update","phase":"replace"}"#
        );

        assert_eq!(
            serde_json::to_string(&UpdateState {
                target_application: None,
                temporary_application: Some(PathBuf::from("/tmp/app-update")),
                trace_context: None,
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
            trace_context: None,
            phase: UpdatePhase::Cleanup,
        };

        let deserialized: UpdateState =
            serde_json::from_str(r#"{"update":"/tmp/app-update","phase":"cleanup"}"#).unwrap();
        assert_eq!(deserialized, update);
    }

    #[test]
    fn test_trace_context_round_trips_through_json() {
        let mut carrier = HashMap::new();
        carrier.insert(
            "traceparent".to_string(),
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string(),
        );
        let state = UpdateState {
            target_application: Some(PathBuf::from("/bin/app")),
            temporary_application: Some(PathBuf::from("/tmp/app-update")),
            trace_context: Some(carrier),
            phase: UpdatePhase::Replace,
        };

        let json = serde_json::to_string(&state).unwrap();
        assert!(
            json.contains("\"trace\":{"),
            "the carrier should serialize under the `trace` key: {json}"
        );

        let restored: UpdateState = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored, state,
            "the trace context should survive a serialize/deserialize round-trip"
        );
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
        let mut carrier = HashMap::new();
        carrier.insert("traceparent".to_string(), "abc".to_string());
        let update = UpdateState {
            target_application: Some(PathBuf::from("/bin/app")),
            temporary_application: Some(PathBuf::from("/tmp/app-update")),
            trace_context: Some(carrier),
            phase: UpdatePhase::Replace,
        };

        let new_update = update.for_phase(UpdatePhase::Cleanup);
        assert_eq!(new_update.target_application, update.target_application);
        assert_eq!(
            new_update.temporary_application,
            update.temporary_application
        );
        assert_eq!(
            new_update.trace_context, update.trace_context,
            "the trace context should be carried forward to the next phase"
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

/// End-to-end check that the active trace context survives a (simulated)
/// relaunch when the `opentelemetry` feature is enabled: capture it under one
/// span, round-trip the state through JSON as it would cross the process
/// boundary, then adopt it under a fresh span and confirm the trace continues.
#[cfg(all(test, feature = "opentelemetry"))]
mod opentelemetry_tests {
    use super::*;
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_subscriber::prelude::*;

    /// The trace id embedded in a W3C `traceparent` header
    /// (`00-<trace id>-<span id>-<flags>`).
    fn trace_id_of(carrier: &HashMap<String, String>) -> String {
        carrier
            .get("traceparent")
            .expect("the W3C propagator should emit a traceparent header")
            .split('-')
            .nth(1)
            .expect("a traceparent has a trace-id segment")
            .to_string()
    }

    fn subscriber() -> impl tracing::Subscriber {
        let tracer = SdkTracerProvider::builder()
            .build()
            .tracer("update-rs-test");
        tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer))
    }

    #[test]
    fn trace_context_continues_across_a_relaunch() {
        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

        // Phase N captures the context of the currently active span.
        let captured = tracing::subscriber::with_default(subscriber(), || {
            let span = tracing::info_span!("phase-n");
            let _enter = span.enter();
            let mut state = UpdateState {
                phase: UpdatePhase::Replace,
                ..Default::default()
            };
            state.capture_trace_context();
            state
        });

        let carrier = captured
            .trace_context
            .clone()
            .expect("an active span should produce a trace context");
        let expected_trace_id = trace_id_of(&carrier);

        // The state crosses the process boundary as JSON.
        let json = serde_json::to_string(&captured).unwrap();
        let resumed: UpdateState = serde_json::from_str(&json).unwrap();

        // Phase N+1 adopts the carried context onto a fresh span *before*
        // entering it (mirroring `UpdateManager::resume`); a span opened within
        // it then belongs to the same trace, which we observe by re-capturing.
        let continued_trace_id = tracing::subscriber::with_default(subscriber(), || {
            let span = tracing::info_span!("phase-n-plus-1");
            resumed.adopt_trace_context(&span);
            span.in_scope(|| {
                let mut next = UpdateState {
                    phase: UpdatePhase::Cleanup,
                    ..Default::default()
                };
                next.capture_trace_context();
                trace_id_of(
                    &next
                        .trace_context
                        .expect("the adopted span should produce a trace context"),
                )
            })
        });

        assert_eq!(
            continued_trace_id, expected_trace_id,
            "the resumed phase should continue the trace captured before the relaunch"
        );
    }
}
