//! Display rows: trailing void copy and short_kind titles.

use health_core::display::{chart_row, event_rows, kind_rows, EventRow as DisplayEvent};
use health_core::{short_kind, EventRow, KindInfo, ObservationRow};

fn projected(id: &str, event_type: &str, retracted: bool, superseded_by: Option<&str>) -> EventRow {
    EventRow {
        id: id.into(),
        ts: "2024-01-15T12:00:00.000000000Z".into(),
        dev: "manual".into(),
        event_type: event_type.into(),
        body: Default::default(),
        superseded_by: superseded_by.map(str::to_string),
        retracted,
    }
}

#[test]
fn event_row_trailing_retracted_and_superseded() {
    let retracted = event_rows(&[projected("a", "note", true, None)]);
    assert_eq!(retracted[0].id, "a");
    assert_eq!(retracted[0].title, "note");
    assert_eq!(
        retracted[0].subtitle.as_deref(),
        Some("manual · 2024-01-15T12:00:00.000000000Z")
    );
    assert_eq!(retracted[0].trailing.as_deref(), Some("retracted"));

    let superseded = event_rows(&[projected("b", "note", false, Some("c"))]);
    assert_eq!(superseded[0].trailing.as_deref(), Some("superseded"));

    let current = event_rows(&[projected("d", "observation", false, None)]);
    assert_eq!(current[0].trailing, None);
}

#[test]
fn retracted_wins_over_superseded_trailing() {
    let rows = event_rows(&[projected("x", "note", true, Some("y"))]);
    assert_eq!(rows[0].trailing.as_deref(), Some("retracted"));
}

#[test]
fn kind_row_title_is_short_kind() {
    let kind = "HKQuantityTypeIdentifierStepCount";
    assert_eq!(short_kind(kind), "StepCount");
    let info = KindInfo {
        kind: kind.into(),
        table: "observations".into(),
        n: 12,
        from: "2024-01-01".into(),
        to: "2024-01-02".into(),
        unit: "count".into(),
        sources: vec!["Apple Watch".into()],
        preferred: "Apple Watch".into(),
        overlaps: false,
    };
    let rows = kind_rows(&[info]);
    assert_eq!(rows[0].id, kind);
    assert_eq!(rows[0].title, "StepCount");
    assert_eq!(rows[0].subtitle.as_deref(), Some("count"));
    assert_eq!(rows[0].trailing.as_deref(), Some("12"));
}

#[test]
fn chart_row_uses_short_kind_title() {
    let kind = "HKQuantityTypeIdentifierHeartRate";
    let obs = ObservationRow {
        dedup_key: "k1".into(),
        n: 1,
        kind: kind.into(),
        source: "Apple Watch".into(),
        source_version: None,
        device: None,
        start_ts: "2024-06-01T08:00:00.000000000Z".into(),
        end_ts: "2024-06-01T08:00:00.000000000Z".into(),
        start_offset: None,
        end_offset: None,
        unit: Some("count/min".into()),
        value: Some("72".into()),
        metadata: Default::default(),
        blob_sha256: None,
        event_id: None,
    };
    let row = chart_row(kind, &[obs]);
    assert_eq!(row.title, "HeartRate");
    assert_eq!(row.subtitle.as_deref(), Some("2024-06-01"));
    assert_eq!(row.unit.as_deref(), Some("count/min"));
    assert_eq!(row.latest.as_deref(), Some("72"));
    assert_eq!(row.values, vec![72.0]);
}

#[test]
fn display_event_row_is_not_the_projection_row() {
    let _ = DisplayEvent {
        id: "id".into(),
        title: "note".into(),
        subtitle: None,
        trailing: Some("retracted".into()),
    };
}
