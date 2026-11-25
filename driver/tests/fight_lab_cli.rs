//! End-to-end contract coverage for `aftermine-driver fight-lab`.

#[path = "support/cli.rs"]
mod cli;

use cli::{driver, refused, succeeded};
use serde_json::Value;

#[test]
fn chosen_kinds_fight_at_each_budget_and_report_as_json() {
    let json: Value = serde_json::from_str(&succeeded(driver().args([
        "fight-lab",
        "--kinds",
        "lancer,sentinel",
        "--budgets",
        "200,440",
        "--no-mixed",
        "--json",
    ])))
    .unwrap();
    assert_eq!(
        json["matrix"]["kinds"],
        serde_json::json!(["lancer", "sentinel"])
    );
    assert_eq!(json["matrix"]["budgets"], serde_json::json!([200, 440]));
    let lancers = &json["matrix"]["trades"][1][0][1];
    assert!(
        lancers["killed"].as_u64() > lancers["lost"].as_u64(),
        "{lancers}"
    );
    assert_eq!(json["mixed"], serde_json::json!([]));
}

#[test]
fn an_unknown_kind_is_refused() {
    let error = refused(driver().args(["fight-lab", "--kinds", "lancer,dragon"]));
    assert!(error.contains("\"dragon\" is not a unit kind"), "{error}");
}

#[test]
fn the_checked_in_counter_chart_is_what_the_lab_measures() {
    let report = succeeded(driver().args(["fight-lab", "--check"]));
    assert!(report.contains("the counter chart is current"), "{report}");
}
