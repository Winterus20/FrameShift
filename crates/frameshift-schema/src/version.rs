use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Current project schema version (`0.2.0`).
pub const CURRENT_SCHEMA_VERSION: SchemaVersion = SchemaVersion {
    major: 0,
    minor: 2,
    patch: 0,
};

/// Semantic version for persisted FrameShift project files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl SchemaVersion {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Returns whether a persisted project version can be loaded by this build.
    ///
    /// Compatibility rule (ADR-0004): the major version must match exactly and
    /// the incoming minor version must be less than or equal to the current
    /// minor version. Older minors stay loadable because every optional field
    /// added in a minor bump is covered by `#[serde(default)]`; newer minors may
    /// carry fields this build does not understand, so they are rejected.
    /// The patch component never affects compatibility: SemVer patches fix bugs
    /// without changing the persisted shape, so any patch is accepted once the
    /// major/minor rule passes.
    pub fn is_compatible_with_current(self) -> bool {
        self.major == CURRENT_SCHEMA_VERSION.major && self.minor <= CURRENT_SCHEMA_VERSION.minor
    }
}

impl Default for SchemaVersion {
    fn default() -> Self {
        CURRENT_SCHEMA_VERSION
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for SchemaVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('.');
        let major = parts
            .next()
            .ok_or_else(|| format!("missing major version in '{s}'"))?
            .parse::<u16>()
            .map_err(|e| format!("invalid major version in '{s}': {e}"))?;
        let minor = parts
            .next()
            .ok_or_else(|| format!("missing minor version in '{s}'"))?
            .parse::<u16>()
            .map_err(|e| format!("invalid minor version in '{s}': {e}"))?;
        let patch = parts
            .next()
            .ok_or_else(|| format!("missing patch version in '{s}'"))?
            .parse::<u16>()
            .map_err(|e| format!("invalid patch version in '{s}': {e}"))?;
        if parts.next().is_some() {
            return Err(format!("unexpected extra version segments in '{s}'"));
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl Serialize for SchemaVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<SchemaVersion>().map_err(serde::de::Error::custom)
    }
}
