#![no_std]

//! A tracing-compatible facade for defmt.
//!
//! This crate provides macros that mimic the `tracing` crate's API but expand to `defmt` macros.
//! This allows using `defmt`'s efficient logging with code written for `tracing` (mostly).

pub use defmt;
pub use tracing_defmt_macros::{debug, error, info, instrument, trace, warn};

/// Wrapper types to support `tracing::field::debug` and `tracing::field::display`.
pub mod field {
    /// A wrapper that implements `defmt::Format` using `core::fmt::Debug`.
    pub struct DebugValue<T>(pub T);

    impl<T: core::fmt::Debug> defmt::Format for DebugValue<T> {
        fn format(&self, fmt: defmt::Formatter) {
            // Use Debug2Format to use the Debug implementation
            defmt::write!(fmt, "{}", defmt::Debug2Format(&self.0))
        }
    }

    /// Wraps a value to be formatted via `Debug`.
    pub fn debug<T>(t: T) -> DebugValue<T> {
        DebugValue(t)
    }

    /// A wrapper that implements `defmt::Format` using `core::fmt::Display`.
    pub struct DisplayValue<T>(pub T);

    impl<T: core::fmt::Display> defmt::Format for DisplayValue<T> {
        fn format(&self, fmt: defmt::Formatter) {
            // Use Display2Format to use the Display implementation
            defmt::write!(fmt, "{}", defmt::Display2Format(&self.0))
        }
    }

    /// Wraps a value to be formatted via `Display`.
    pub fn display<T>(t: T) -> DisplayValue<T> {
        DisplayValue(t)
    }
}

/// Describes the level of verbosity of a span or event.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Designates very serious errors.
    Error = 1,
    /// Designates hazardous situations.
    Warn = 2,
    /// Designates useful information.
    Info = 3,
    /// Designates lower priority information.
    Debug = 4,
    /// Designates very low priority, often extremely verbose, information.
    Trace = 5,
}

impl Level {
    pub const TRACE: Self = Self::Trace;
    pub const DEBUG: Self = Self::Debug;
    pub const INFO: Self = Self::Info;
    pub const WARN: Self = Self::Warn;
    pub const ERROR: Self = Self::Error;
}

// Initial placeholder for `event!` which tracing uses extensively.
#[macro_export]
macro_rules! event {
    (target: $target:expr, $lvl:expr, $($args:tt)*) => {
        // We currently ignore target
        $crate::event!($lvl, $($args)*)
    };
    ($lvl:expr, $($args:tt)*) => {
        match $lvl {
            $crate::Level::Error => $crate::error!($($args)*),
            $crate::Level::Warn => $crate::warn!($($args)*),
            $crate::Level::Info => $crate::info!($($args)*),
            $crate::Level::Debug => $crate::debug!($($args)*),
            $crate::Level::Trace => $crate::trace!($($args)*),
        }
    };
}

/// A dummy Span to satisfy the tracing API.
///
/// `defmt` does not natively support nested spans with attached key-value pairs
/// in the same way `tracing` does. This struct allows code that creates spans
/// to compile, but span context is currently not propagated or logged.
#[derive(Clone, Debug, Default)]
pub struct Span;

impl Span {
    pub const fn none() -> Self {
        Span
    }

    pub fn current() -> Self {
        Span
    }

    pub fn enter(&self) -> Entered {
        Entered
    }

    pub fn record(&self, _field: &str, _value: &dyn core::fmt::Debug) -> &Self {
        self
    }

    pub fn is_disabled(&self) -> bool {
        false
    }

    pub fn is_none(&self) -> bool {
        false
    }

    pub fn in_scope<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        f()
    }
}

pub struct Entered;

impl Drop for Entered {
    fn drop(&mut self) {}
}

#[macro_export]
macro_rules! span {
    (target: $target:expr, $lvl:expr, $($args:tt)*) => {
        $crate::Span
    };
    ($lvl:expr, $($args:tt)*) => {
        $crate::Span
    };
}

#[macro_export]
macro_rules! trace_span {
    ($($args:tt)*) => {
        $crate::span!($crate::Level::Trace, $($args)*)
    };
}

#[macro_export]
macro_rules! debug_span {
    ($($args:tt)*) => {
        $crate::span!($crate::Level::Debug, $($args)*)
    };
}

#[macro_export]
macro_rules! info_span {
    ($($args:tt)*) => {
        $crate::span!($crate::Level::Info, $($args)*)
    };
}

#[macro_export]
macro_rules! warn_span {
    ($($args:tt)*) => {
        $crate::span!($crate::Level::Warn, $($args)*)
    };
}

#[macro_export]
macro_rules! error_span {
    ($($args:tt)*) => {
        $crate::span!($crate::Level::Error, $($args)*)
    };
}
