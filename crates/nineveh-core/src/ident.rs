use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

/// A Move identifier: a module, struct, field or variant name.
///
/// Identifiers are cheap to clone, since decoded values carry a field name per field.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identifier(Arc<str>);

impl Identifier {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// An identifier from a string literal the caller knows is valid, such as a
    /// framework module name.
    ///
    /// Validity is checked in debug builds only; use [`FromStr`] for anything that
    /// isn't a literal.
    #[must_use]
    pub fn from_static(s: &'static str) -> Self {
        debug_assert!(Self::is_valid(s), "invalid identifier literal {s:?}");
        Self(Arc::from(s))
    }

    /// Whether `s` is a valid Move identifier: `[A-Za-z_][A-Za-z0-9_]*`, but not `_`
    /// alone.
    #[must_use]
    pub fn is_valid(s: &str) -> bool {
        let mut chars = s.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        match first {
            'a'..='z' | 'A'..='Z' => rest_ok,
            '_' => s.len() > 1 && rest_ok,
            _ => false,
        }
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl AsRef<str> for Identifier {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for Identifier {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

/// Text that isn't a valid Move identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid Move identifier `{0}`")]
pub struct InvalidIdentifier(pub String);

impl FromStr for Identifier {
    type Err = InvalidIdentifier;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if Self::is_valid(s) {
            Ok(Self(Arc::from(s)))
        } else {
            Err(InvalidIdentifier(s.to_owned()))
        }
    }
}

impl serde::Serialize for Identifier {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Identifier {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_move_identifiers() {
        for ok in ["vault", "DepositEvent", "_x", "__variant__", "a1_b2"] {
            assert!(Identifier::is_valid(ok), "{ok:?}");
        }
    }

    #[test]
    fn rejects_non_identifiers() {
        for bad in ["", "_", "1a", "a-b", "a b", "a::b", "é"] {
            assert!(bad.parse::<Identifier>().is_err(), "{bad:?}");
        }
    }
}
