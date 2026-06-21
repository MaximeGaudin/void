use std::collections::HashSet;
use std::fmt;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

static INTERN_POOL: LazyLock<Mutex<HashSet<&'static str>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn intern(s: &str) -> &'static str {
    let mut pool = INTERN_POOL.lock().unwrap_or_else(|e| e.into_inner());
    for &existing in pool.iter() {
        if existing == s {
            return existing;
        }
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    pool.insert(leaked);
    leaked
}

/// Registry-agnostic connector identity stored as an interned string.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectorType(&'static str);

impl ConnectorType {
    pub const fn from_static(s: &'static str) -> Self {
        Self(s)
    }

    pub fn new(s: &str) -> Self {
        Self(intern(s))
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Debug for ConnectorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for ConnectorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ConnectorType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_str().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConnectorType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(ConnectorType::new(&s))
    }
}
