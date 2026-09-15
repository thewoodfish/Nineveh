//! The server-side stream filter for a project (ADR 0004).

use nineveh_config::{Input, Project};
use nineveh_core::StructName;
use nineveh_decode::TypeMatcher;
use nineveh_proto::indexer::{
    ApiFilter, BooleanTransactionFilter, EventFilter, LogicalOrFilters, MoveStructTagFilter,
    api_filter, boolean_transaction_filter,
};

/// The filter to stream `project` with: the OR of its event types when every source is
/// an `event:` source, and `None` (stream everything) otherwise.
///
/// Write-set changes can't be filtered server-side, so a project with a `resource:` or
/// `table:` source streams unfiltered: any filter would under-cover it (ADR 0004).
/// An event filter names the struct without type arguments, so it can match more than
/// the source does, never less; the decoder selects exactly.
#[must_use]
pub fn stream_filter(project: &Project) -> Option<BooleanTransactionFilter> {
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
    names.sort();
    names.dedup();
    let filters = names.into_iter().map(event_type).collect();
    Some(BooleanTransactionFilter {
        filter: Some(boolean_transaction_filter::Filter::LogicalOr(
            LogicalOrFilters { filters },
        )),
    })
}

fn event_type(name: &StructName) -> BooleanTransactionFilter {
    BooleanTransactionFilter {
        filter: Some(boolean_transaction_filter::Filter::ApiFilter(ApiFilter {
            filter: Some(api_filter::Filter::EventFilter(EventFilter {
                struct_type: Some(MoveStructTagFilter {
                    // The stream renders type addresses with leading zeros stripped.
                    // The server matches this form whether it compares strings as
                    // rendered or normalizes both sides; the live test in
                    // tests/filter.rs checks it on an address whose first nibble is 0.
                    address: Some(name.address.to_short_string()),
                    module: Some(name.module.as_str().to_owned()),
                    name: Some(name.name.as_str().to_owned()),
                }),
                data_substring_filter: None,
            })),
        })),
    }
}
