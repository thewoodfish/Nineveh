//! Webhooks may subscribe to a table either file declares.
//!
//! `nineveh.yaml` keeps the mirrors and the logs, a `.nineveh.ts` file keeps the reduce
//! tables, and `webhooks:` is written in the YAML — so the set a subscription is checked
//! against isn't complete until the two halves are merged. Checking it any earlier
//! rejects a table that is really there, which is what used to happen to every table a
//! reducers file declares.

#![allow(clippy::panic, reason = "a failing merge is reported with its text")]

const YAML: &str = r#"
name: market
network: testnet
start_version: 1
reducers: ./market.nineveh.ts

sources:
  cancelled: { event: "0x1::market::Cancelled" }

state:
  cancellations: { log: cancelled }

webhooks:
  backend:
    url: https://example.com/hook
    on: [cancelled_per_id.inserted, cancellations.inserted]
"#;

const DSL: &str = r"
export const cancelled_per_id = table({
  key:     { id: u64 },
  columns: { count: u64.default(0) },
})

on(cancelled, (c) => {
  cancelled_per_id.row(c.id).count += 1
})
";

/// Replace the `on:` list, so each case differs only in what it subscribes to.
fn yaml_subscribing_to(on: &str) -> String {
    YAML.replace(
        "on: [cancelled_per_id.inserted, cancellations.inserted]",
        &format!("on: [{on}]"),
    )
}

fn merged(yaml: &str, dsl: &str) -> Result<nineveh_config::Config, String> {
    let config = nineveh_config::parse(yaml)
        .map_err(|d| d.render_files(&[("nineveh.yaml", yaml), ("market.nineveh.ts", dsl)]))?;
    nineveh_dsl::merge(config, dsl)
        .map_err(|d| d.render_files(&[("nineveh.yaml", yaml), ("market.nineveh.ts", dsl)]))
}

#[test]
fn a_webhook_can_subscribe_to_a_table_the_reducers_file_declares() {
    let config = merged(YAML, DSL).unwrap_or_else(|rendered| panic!("{rendered}"));

    let hook = config
        .webhooks
        .first()
        .unwrap_or_else(|| panic!("the config declares one webhook"));
    let tables: Vec<&str> = hook.on.iter().map(|s| s.table.as_str()).collect();
    assert_eq!(
        tables,
        ["cancelled_per_id", "cancellations"],
        "a subscription to a DSL table should survive validation alongside a YAML one"
    );
}

#[test]
fn parse_alone_accepts_a_subscription_it_cannot_yet_check() {
    // The YAML half names a table it doesn't declare, which is legal until the merge:
    // deciding at this point is exactly the bug.
    nineveh_config::parse(YAML).unwrap_or_else(|d| panic!("{}", d.render("nineveh.yaml", YAML)));
}

#[test]
fn a_table_neither_file_declares_is_still_rejected() {
    let yaml = yaml_subscribing_to("cancelled_per_di.inserted");
    let rendered = merged(&yaml, DSL)
        .err()
        .unwrap_or_else(|| panic!("a subscription to an undeclared table should fail"));

    assert!(
        rendered.contains("unknown state table `cancelled_per_di`"),
        "should name the table it can't find: {rendered}"
    );
    assert!(
        rendered.contains("did you mean `cancelled_per_id`?"),
        "a near miss should suggest the DSL's table, now that it can see it: {rendered}"
    );
    assert!(
        rendered.contains("nineveh.yaml"),
        "the subscription is written in the YAML, so that's where it should point: \
         {rendered}"
    );
}

#[test]
fn a_yaml_only_project_reports_an_unknown_table_as_before() {
    let yaml = r#"
name: market
network: testnet
start_version: 1

sources:
  cancelled: { event: "0x1::market::Cancelled" }

state:
  cancellations: { log: cancelled }

webhooks:
  backend:
    url: https://example.com/hook
    on: [cancellationz.inserted]
"#;
    let rendered = nineveh_config::parse(yaml).map_or_else(
        |d| d.render("nineveh.yaml", yaml),
        |_| panic!("a project with no reducers file is complete, so this should fail"),
    );

    assert!(
        rendered.contains("unknown state table `cancellationz`"),
        "{rendered}"
    );
    assert!(
        rendered.contains("did you mean `cancellations`?"),
        "{rendered}"
    );
}

#[test]
fn a_bad_subscription_is_reported_with_the_dsl_problems_beside_it() {
    // One run, both files' problems: the DSL collides with a YAML table, and the YAML
    // subscribes to nothing that exists.
    let dsl = DSL.replace("cancelled_per_id", "cancellations");
    let yaml = yaml_subscribing_to("nope.inserted");
    let rendered = merged(&yaml, &dsl)
        .err()
        .unwrap_or_else(|| panic!("both halves are wrong, so this should fail"));

    assert!(
        rendered.contains("is already a table in nineveh.yaml"),
        "the DSL's collision should still be reported: {rendered}"
    );
    assert!(
        rendered.contains("unknown state table `nope`"),
        "and the webhook's problem with it, in the same run: {rendered}"
    );
}
