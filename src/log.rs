//! Internal logging facade.
//!
//! The crate's diagnostics are opt-in behind the `tracing` feature. These
//! macros forward to the matching [`tracing`] macros when the feature is
//! enabled, and otherwise expand to a call that still evaluates (and therefore
//! type-checks) the format arguments before discarding them. That keeps the
//! crate dependency-free and warning-clean by default, while every call site can
//! be written exactly as it would be with `tracing`.

#[cfg(feature = "tracing")]
macro_rules! debug {
    ($($arg:tt)*) => { ::tracing::debug!($($arg)*) };
}

#[cfg(feature = "tracing")]
macro_rules! info {
    ($($arg:tt)*) => { ::tracing::info!($($arg)*) };
}

#[cfg(not(feature = "tracing"))]
macro_rules! debug {
    ($($arg:tt)*) => { $crate::log::discard(::core::format_args!($($arg)*)) };
}

#[cfg(not(feature = "tracing"))]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::discard(::core::format_args!($($arg)*)) };
}

pub(crate) use {debug, info};

/// Consume formatted log arguments without emitting anything. Used by the no-op
/// logging macros when the `tracing` feature is disabled; `#[inline(always)]`
/// means the call (and the `Arguments` it builds) compiles away entirely while
/// still marking the referenced values as used.
#[cfg(not(feature = "tracing"))]
#[inline(always)]
pub(crate) fn discard(_: ::core::fmt::Arguments<'_>) {}
