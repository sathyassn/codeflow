//! The limits registry: every limit, declared once (TSK-1299).
//!
//! A timeout, retry, count, size, wait, shard or parallel number or word
//! limit has one entry in the registry, with its key, default, bounds, class
//! and reason. Code reads the value from the entry and never from a literal;
//! `crates/codeflow-core/tests/limit_literals.rs` fails on a literal limit
//! anywhere else in product code. The policy schema and `docs/limits.md` are
//! generated from the entries.
//!
//! The registry is its own crate because `codeflow-core` and
//! `codeflow-present` both read it and neither depends on the other.
//!
//! The three classes:
//!
//! - [`LimitClass::Operational`] tunes how long or how much work may take.
//!   The project sets the value. It has a floor where correctness needs one
//!   and no ceiling, beyond what deadline arithmetic can hold.
//! - [`LimitClass::Security`] keeps a control fail-closed. The floor and the
//!   buffered ceiling live here: the project can tighten the value down to
//!   the floor, and raise it to the ceiling where that is safe.
//! - [`LimitClass::Guidance`] is a value derived from memory, disk or load,
//!   or a shipped wait default. Its consumer may only report or warn: it never
//!   refuses, queues, waits or lowers a value the project set (ADR-0083).
//!
//! An entry on [`Route::Fixed`] is not settable, and its reason says why.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

mod registry;

pub use registry::*;

/// The unit of a time limit counted in seconds.
pub const SECONDS: &str = "seconds";
/// The unit of a time limit counted in milliseconds.
pub const MILLISECONDS: &str = "milliseconds";
/// The unit of a size limit.
pub const BYTES: &str = "bytes";

/// What a limit protects, which decides how far a project can move it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitClass {
    /// Tunes how long or how much work may take; no ceiling.
    Operational,
    /// Keeps a control fail-closed; a floor and a buffered ceiling.
    Security,
    /// Reported or warned about only; no refusal and no ceiling.
    Guidance,
}

impl LimitClass {
    /// The class as the limits page names it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Operational => "operational",
            Self::Security => "security",
            Self::Guidance => "guidance",
        }
    }
}

/// Where a project sets a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// A key of `.codeflow/policy.json`.
    Policy,
    /// A key of the test configuration, per target or under `execution`.
    TestConfig,
    /// A key of `.codeflow/ci-wait.json`.
    CiWait,
    /// A key of the present service's own configuration file.
    PresentConfig,
    /// A flag on the command that runs the work.
    CommandLine,
    /// Not settable; the entry's reason says why.
    Fixed,
}

impl Route {
    /// Where to set the limit, as the limits page names it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Policy => "`.codeflow/policy.json`",
            Self::TestConfig => "test configuration",
            Self::CiWait => "`.codeflow/ci-wait.json`",
            Self::PresentConfig => "present configuration",
            Self::CommandLine => "command line",
            Self::Fixed => "not settable",
        }
    }
}

/// One declared limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    /// The key that sets it on its [`Route`], or `None` when nothing does.
    pub key: Option<&'static str>,
    /// Where the key lives.
    pub route: Route,
    /// What it limits, for messages and docs.
    pub name: &'static str,
    /// The unit `default`, `min` and `max` are in.
    pub unit: &'static str,
    /// The shipped value.
    pub default: u64,
    /// The smallest accepted value.
    pub min: u64,
    /// The largest accepted value; `None` when only the integer type
    /// bounds it.
    pub max: Option<u64>,
    pub class: LimitClass,
    /// Why the default is what it is, or why the limit is not settable.
    pub reason: &'static str,
    /// What a guidance entry's consumer reports; empty on other classes.
    pub warning: &'static str,
}

/// An operational policy key: a correctness floor and no ceiling.
#[must_use]
pub const fn operational(
    key: &'static str,
    name: &'static str,
    unit: &'static str,
    default: u64,
    min: u64,
    reason: &'static str,
) -> Limit {
    Limit {
        key: Some(key),
        route: Route::Policy,
        name,
        unit,
        default,
        min,
        max: None,
        class: LimitClass::Operational,
        reason,
        warning: "",
    }
}

/// An operational policy key held in a narrower integer than `u64`: only the
/// number's own width bounds it.
#[must_use]
pub const fn operational_u32(
    key: &'static str,
    name: &'static str,
    unit: &'static str,
    default: u64,
    min: u64,
    reason: &'static str,
) -> Limit {
    Limit {
        max: Some(u32::MAX as u64),
        ..operational(key, name, unit, default, min, reason)
    }
}

/// A security policy key: the project moves it between `min` and `max`.
#[must_use]
pub const fn security(
    key: &'static str,
    name: &'static str,
    unit: &'static str,
    default: u64,
    min: u64,
    max: u64,
    reason: &'static str,
) -> Limit {
    Limit {
        key: Some(key),
        route: Route::Policy,
        name,
        unit,
        default,
        min,
        max: Some(max),
        class: LimitClass::Security,
        reason,
        warning: "",
    }
}

/// A guidance entry on `route`: reported only, never a refusal or a ceiling.
#[must_use]
pub const fn guidance(
    key: &'static str,
    route: Route,
    name: &'static str,
    unit: &'static str,
    default: u64,
    reason: &'static str,
    warning: &'static str,
) -> Limit {
    Limit {
        key: Some(key),
        route,
        name,
        unit,
        default,
        min: 0,
        max: None,
        class: LimitClass::Guidance,
        reason,
        warning,
    }
}

/// A limit a project sets outside the policy file.
#[must_use]
pub const fn elsewhere(
    key: &'static str,
    route: Route,
    name: &'static str,
    unit: &'static str,
    default: u64,
    bounds: (u64, Option<u64>),
    reason: &'static str,
) -> Limit {
    Limit {
        key: Some(key),
        route,
        name,
        unit,
        default,
        min: bounds.0,
        max: bounds.1,
        class: LimitClass::Operational,
        reason,
        warning: "",
    }
}

/// A limit no project can set; `reason` says why.
#[must_use]
pub const fn fixed(
    class: LimitClass,
    name: &'static str,
    unit: &'static str,
    default: u64,
    reason: &'static str,
) -> Limit {
    Limit {
        key: None,
        route: Route::Fixed,
        name,
        unit,
        default,
        min: default,
        max: Some(default),
        class,
        reason,
        warning: "",
    }
}

/// The values the project set, by policy key. The first install wins, so a
/// process keeps the values of the authority it resolved first.
static PROJECT_VALUES: OnceLock<BTreeMap<&'static str, u64>> = OnceLock::new();

/// Give the process the values its project set. Keys the registry does not
/// hold and values outside their bounds are dropped, so nothing past this
/// point can see one; policy validation has already refused them by name.
/// Whether these values took effect: a second call changes nothing.
pub fn install<'a>(values: impl IntoIterator<Item = (&'a str, u64)>) -> bool {
    let accepted = values
        .into_iter()
        .filter_map(|(key, value)| {
            let limit = for_key(key)?;
            limit.accepts(value).then_some((limit.key?, value))
        })
        .collect();
    PROJECT_VALUES.set(accepted).is_ok()
}

impl Limit {
    /// The effective value: what the project set, else the default.
    #[must_use]
    pub fn value(&self) -> u64 {
        self.key
            .and_then(|key| PROJECT_VALUES.get()?.get(key).copied())
            .unwrap_or(self.default)
    }

    /// [`Self::value`] as a count or size in memory.
    #[must_use]
    pub fn count(&self) -> usize {
        usize::try_from(self.value()).unwrap_or(usize::MAX)
    }

    /// [`Self::value`] for a 32-bit consumer.
    #[must_use]
    pub fn small(&self) -> u32 {
        u32::try_from(self.value()).unwrap_or(u32::MAX)
    }

    /// [`Self::value`] as a duration, in the entry's unit.
    #[must_use]
    pub fn wait(&self) -> Duration {
        if self.unit == MILLISECONDS {
            Duration::from_millis(self.value())
        } else {
            Duration::from_secs(self.value())
        }
    }

    /// The default as a `usize`, for a constant.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn default_count(&self) -> usize {
        self.default as usize
    }

    /// The default as a `u32`, for a constant.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn default_small(&self) -> u32 {
        self.default as u32
    }

    /// The largest accepted value, for a constant.
    #[must_use]
    pub const fn most(&self) -> u64 {
        match self.max {
            Some(max) => max,
            None => u64::MAX,
        }
    }

    /// Whether a project may set `value`.
    #[must_use]
    pub fn accepts(&self, value: u64) -> bool {
        value >= self.min && self.max.is_none_or(|max| value <= max)
    }

    /// The accepted values, for messages.
    #[must_use]
    pub fn bounds_text(&self) -> String {
        let max = self.max.unwrap_or(u64::MAX);
        format!("a whole number of {} from {} to {max}", self.unit, self.min)
    }

    /// The accepted values, for the policy schema and the limits page.
    #[must_use]
    pub fn valid_text(&self) -> String {
        match self.max {
            Some(max) if max >= u64::from(u32::MAX) => format!(
                "a whole number of {}, at least {}; no ceiling beyond what the number can hold",
                self.unit, self.min
            ),
            Some(max) => format!("a whole number of {} from {} to {max}", self.unit, self.min),
            None => format!(
                "a whole number of {}, at least {}; no ceiling beyond what the number can hold",
                self.unit, self.min
            ),
        }
    }
}

/// The declared limit a policy key sets, if any.
#[must_use]
pub fn for_key(key: &str) -> Option<&'static Limit> {
    LIMITS
        .iter()
        .find(|limit| limit.route == Route::Policy && limit.key == Some(key))
}

#[cfg(test)]
mod tests;
