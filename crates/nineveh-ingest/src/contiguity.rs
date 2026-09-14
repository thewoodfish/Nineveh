//! Checks that the stream delivers every version exactly once, in order.
//!
//! This is the invariant that makes the version cursor trustworthy. Everything
//! downstream assumes that once version `v` has been handed out, every version before
//! it has been handed out too. The check is pure, so it can be tested without a network.

use std::ops::RangeInclusive;

use nineveh_core::Version;

use crate::IngestError;

#[derive(Debug)]
pub(crate) struct Contiguity {
    /// The next version the stream must account for.
    next: Version,
    /// Whether a server-side filter is active. When it is, the stream skips versions
    /// that don't match. Coverage then comes from the processed range, not from the
    /// transactions themselves.
    filtered: bool,
}

impl Contiguity {
    pub(crate) fn new(starting_version: Version, filtered: bool) -> Self {
        Self {
            next: starting_version,
            filtered,
        }
    }

    /// The next version the stream owes us.
    pub(crate) fn next(&self) -> Version {
        self.next
    }

    /// Validate one response and advance past it.
    ///
    /// The check is transactional: if it fails, the cursor stays where it was.
    pub(crate) fn advance(
        &mut self,
        versions: impl IntoIterator<Item = Version>,
        processed_range: Option<RangeInclusive<Version>>,
    ) -> Result<(), IngestError> {
        let start = self.next;
        let mut next = start;

        if let Some(range) = &processed_range
            && *range.start() != start
        {
            return Err(IngestError::RangeMismatch {
                expected: start,
                first: *range.start(),
                last: *range.end(),
            });
        }

        for version in versions {
            if version < next {
                return Err(IngestError::OutOfOrder {
                    expected: next,
                    got: version,
                });
            }
            if version > next && !self.filtered {
                return Err(IngestError::Gap {
                    expected: next,
                    got: version,
                });
            }
            if let Some(range) = &processed_range
                && !range.contains(&version)
            {
                return Err(IngestError::OutsideRange {
                    version,
                    first: *range.start(),
                    last: *range.end(),
                });
            }
            next = version.next().ok_or(IngestError::VersionOverflow)?;
        }

        // With a filter, the processed range can extend past the last delivered
        // transaction: the server scanned those versions and none of them matched.
        // Without one, every version in the range must have been delivered.
        if let Some(range) = processed_range {
            let after_range = range.end().next().ok_or(IngestError::VersionOverflow)?;
            if !self.filtered && next < after_range {
                return Err(IngestError::Gap {
                    expected: next,
                    got: after_range,
                });
            }
            next = next.max(after_range);
        }

        self.next = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(n: u64) -> Version {
        Version::new(n)
    }

    fn vs(ns: &[u64]) -> Vec<Version> {
        ns.iter().copied().map(Version::new).collect()
    }

    #[test]
    fn unfiltered_accepts_dense_versions_across_responses() {
        let mut c = Contiguity::new(v(10), false);
        c.advance(vs(&[10, 11, 12]), None).unwrap();
        c.advance(vs(&[13]), None).unwrap();
        assert_eq!(c.next(), v(14));
    }

    #[test]
    fn unfiltered_rejects_a_skipped_version() {
        let mut c = Contiguity::new(v(10), false);
        let err = c.advance(vs(&[10, 12]), None).unwrap_err();
        assert!(
            matches!(err, IngestError::Gap { expected, got } if expected == v(11) && got == v(12))
        );
    }

    #[test]
    fn rejects_repeated_or_backwards_versions() {
        let mut c = Contiguity::new(v(10), true);
        let err = c.advance(vs(&[10, 10]), None).unwrap_err();
        assert!(
            matches!(err, IngestError::OutOfOrder { expected, got } if expected == v(11) && got == v(10))
        );
    }

    #[test]
    fn a_failed_check_leaves_the_cursor_in_place() {
        let mut c = Contiguity::new(v(10), false);
        c.advance(vs(&[10, 11, 13]), None).unwrap_err();
        assert_eq!(c.next(), v(10));
    }

    #[test]
    fn filtered_allows_sparse_versions() {
        let mut c = Contiguity::new(v(10), true);
        c.advance(vs(&[12, 40]), None).unwrap();
        assert_eq!(c.next(), v(41));
    }

    #[test]
    fn processed_range_advances_over_versions_with_no_matches() {
        let mut c = Contiguity::new(v(10), true);
        c.advance(vs(&[15]), Some(v(10)..=v(99))).unwrap();
        c.advance(vs(&[]), Some(v(100)..=v(199))).unwrap();
        assert_eq!(c.next(), v(200));
    }

    #[test]
    fn unfiltered_range_must_be_fully_delivered() {
        let mut c = Contiguity::new(v(10), false);
        let err = c.advance(vs(&[10, 11]), Some(v(10)..=v(12))).unwrap_err();
        assert!(matches!(err, IngestError::Gap { expected, .. } if expected == v(12)));
    }

    #[test]
    fn processed_range_must_start_at_the_cursor() {
        let mut c = Contiguity::new(v(10), true);
        let err = c.advance(vs(&[]), Some(v(11)..=v(20))).unwrap_err();
        assert!(matches!(err, IngestError::RangeMismatch { .. }));
    }

    #[test]
    fn transactions_must_lie_inside_the_processed_range() {
        let mut c = Contiguity::new(v(10), true);
        let err = c.advance(vs(&[25]), Some(v(10)..=v(20))).unwrap_err();
        assert!(matches!(err, IngestError::OutsideRange { .. }));
    }

    #[test]
    fn overflow_is_an_error_not_a_panic() {
        let mut c = Contiguity::new(v(u64::MAX), false);
        let err = c.advance(vs(&[u64::MAX]), None).unwrap_err();
        assert!(matches!(err, IngestError::VersionOverflow));
    }
}
