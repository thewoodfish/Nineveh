//! The server-side stream filter for a project (ADR 0004).

use std::collections::BTreeSet;

use nineveh_config::{Input, Project};
use nineveh_core::StructName;
use nineveh_decode::TypeMatcher;
use nineveh_proto::indexer::{
    ApiFilter, BooleanTransactionFilter, EventFilter, LogicalOrFilters, MoveStructTagFilter,
    api_filter, boolean_transaction_filter,
};
use prost::Message;

/// The largest filter to send. The Transaction Stream refuses a filter over 10,000
/// encoded bytes ("Filter is too complicated", testnet, 2026-09-15: 98 event types from
/// one contract came to 11,781); this leaves a margin.
pub const MAX_FILTER_BYTES: usize = 9_000;

/// The filter to stream `project` with: the OR of its event types when every source is
/// an `event:` source, and `None` (stream everything) otherwise.
///
/// Write-set changes can't be filtered server-side, so a project with a `resource:` or
/// `table:` source streams unfiltered: any filter would under-cover it (ADR 0004).
/// An event filter names the struct without type arguments, so it can match more than
/// the source does, never less; the decoder selects exactly.
///
/// A filter too big for the server matches less precisely instead: every event of
/// each module the event types are in, then every event of each address, and if even
/// that's too big, no filter. Each step matches more, never less.
#[must_use]
pub fn stream_filter(project: &Project) -> Option<BooleanTransactionFilter> {
    stream_filter_within(project, MAX_FILTER_BYTES)
}

/// [`stream_filter`] with a size budget of `max_bytes` encoded bytes.
#[must_use]
pub fn stream_filter_within(
    project: &Project,
    max_bytes: usize,
) -> Option<BooleanTransactionFilter> {
    let mut names: Vec<&StructName> = Vec::new();
    for source in &project.config().sources {
        let input = project
            .source_id(source.name.as_str())
            .and_then(|id| project.input(id))?;
        let Input::Event(matcher) = input else {
            return None;
        };
        names.push(match matcher {
            TypeMatcher::Exact(tag) => &tag.name,
            TypeMatcher::AnyInstance(name) => name,
        });
    }
    if names.is_empty() {
        return None;
    }
    [Precision::Type, Precision::Module, Precision::Address]
        .into_iter()
        .map(|precision| {
            let leaves: BTreeSet<Leaf> = names.iter().map(|n| Leaf::of(n, precision)).collect();
            BooleanTransactionFilter {
                filter: Some(boolean_transaction_filter::Filter::LogicalOr(
                    LogicalOrFilters {
                        filters: leaves.into_iter().map(Leaf::filter).collect(),
                    },
                )),
            }
        })
        .find(|filter| filter.encoded_len() <= max_bytes)
}

/// How much of an event type a filter leaf names.
#[derive(Debug, Clone, Copy)]
enum Precision {
    Type,
    Module,
    Address,
}

/// One event filter: an address, and optionally a module and a struct in it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Leaf {
    address: String,
    module: Option<String>,
    name: Option<String>,
}

impl Leaf {
    fn of(name: &StructName, precision: Precision) -> Self {
        Self {
            // The stream's own rendering, leading zeros stripped. The server normalizes
            // filter addresses (on testnet, 2026-09-15, the short and full forms of a
            // 63-digit address both matched), so either form works; tests/filter.rs has
            // the live check.
            address: name.address.to_short_string(),
            module: match precision {
                Precision::Type | Precision::Module => Some(name.module.as_str().to_owned()),
                Precision::Address => None,
            },
            name: match precision {
                Precision::Type => Some(name.name.as_str().to_owned()),
                Precision::Module | Precision::Address => None,
            },
        }
    }

    fn filter(self) -> BooleanTransactionFilter {
        BooleanTransactionFilter {
            filter: Some(boolean_transaction_filter::Filter::ApiFilter(ApiFilter {
                filter: Some(api_filter::Filter::EventFilter(EventFilter {
                    struct_type: Some(MoveStructTagFilter {
                        address: Some(self.address),
                        module: self.module,
                        name: self.name,
                    }),
                    data_substring_filter: None,
                })),
            })),
        }
    }
}
