//! Crash/resume and claim discipline for the work-queue substrate.
//!
//! Ported from gallery-ml's `cache` tests: reclaim is a run-start call
//! (not implied by opening a connection), a lost claim cannot finish, and
//! cancel release only touches rows this run holds.

use localcore_queue::{
    begin, claimable, configure_connection, enqueue, ensure_table, finish_done, finish_failed,
    finish_skipped, item, mark_stale, mark_stale_for_pack, reclaim_abandoned, release, reset,
    set_content_hash, stats, Queue, WorkState, MAX_RETRIES,
};
use rusqlite::Connection;

const Q: Queue = Queue::new("ml_work", "tag_count");
const FACES: Queue = Queue::new("face_work", "face_count");
const PLACES: Queue = Queue::new("places_work", "place_count");

/// Capability-local "unsupported format" code; the crate stores it blindly.
const UNSUPPORTED: i64 = 3;

fn mem() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    configure_connection(&conn).unwrap();
    ensure_table(&conn, Q).unwrap();
    conn
}

fn claim_and_finish(conn: &Connection, path: &str, pack: &str, count: usize) {
    assert!(begin(conn, Q, path).unwrap(), "{path} was not claimable");
    assert!(finish_done(conn, Q, path, pack, count, Some((10, Some(20)))).unwrap());
}

fn claim_and_fail(conn: &Connection, path: &str, code: i64) {
    assert!(begin(conn, Q, path).unwrap(), "{path} was not claimable");
    assert!(finish_failed(conn, Q, path, code).unwrap());
}

fn claimable_paths(conn: &Connection) -> Vec<String> {
    claimable(conn, Q, 0, None)
        .unwrap()
        .into_iter()
        .map(|i| i.path)
        .collect()
}

#[test]
fn work_states_round_trip() {
    for s in [
        WorkState::Pending,
        WorkState::Hashing,
        WorkState::Done,
        WorkState::Failed,
        WorkState::Stale,
        WorkState::Skipped,
    ] {
        assert_eq!(WorkState::from_i64(s.as_i64()), s);
    }
    assert_eq!(WorkState::from_i64(42), WorkState::Pending);
}

#[test]
fn enqueue_is_idempotent_and_preserves_state() {
    let mut conn = mem();
    assert_eq!(
        enqueue(&mut conn, Q, &["/a.jpg".into(), "/b.jpg".into()]).unwrap(),
        2
    );
    claim_and_finish(&conn, "/a.jpg", "pack-1", 3);
    assert_eq!(
        enqueue(&mut conn, Q, &["/a.jpg".into(), "/c.jpg".into()]).unwrap(),
        1
    );
    assert_eq!(
        item(&conn, Q, "/a.jpg").unwrap().unwrap().state,
        WorkState::Done
    );
}

/// Opening a connection must not reclaim: two capabilities share a file,
/// and a second opener cannot tell a live claim from a crash.
#[test]
fn opening_a_second_handle_leaves_in_flight_rows_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("q.sqlite");
    {
        let mut conn = Connection::open(&path).unwrap();
        configure_connection(&conn).unwrap();
        ensure_table(&conn, Q).unwrap();
        ensure_table(&conn, FACES).unwrap();
        enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();
        enqueue(&mut conn, FACES, &["/b.jpg".into()]).unwrap();
        begin(&conn, Q, "/a.jpg").unwrap();
        begin(&conn, FACES, "/b.jpg").unwrap();
    }
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        item(&conn, Q, "/a.jpg").unwrap().unwrap().state,
        WorkState::Hashing,
        "opening reclaimed a live tagging claim"
    );
    assert_eq!(
        item(&conn, FACES, "/b.jpg").unwrap().unwrap().state,
        WorkState::Hashing,
        "opening reclaimed a live face claim"
    );
    assert!(claimable_paths(&conn).is_empty());
}

/// A row abandoned by a crash is recovered at the next run start — the
/// only moment the answer is knowable.
#[test]
fn an_abandoned_row_is_reclaimed_at_the_next_run_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("q.sqlite");
    {
        let mut conn = Connection::open(&path).unwrap();
        configure_connection(&conn).unwrap();
        ensure_table(&conn, Q).unwrap();
        enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();
        begin(&conn, Q, "/a.jpg").unwrap();
    }
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        item(&conn, Q, "/a.jpg").unwrap().unwrap().state,
        WorkState::Hashing,
        "the open reclaimed, which must not happen"
    );
    assert_eq!(reclaim_abandoned(&conn, Q).unwrap(), 1);
    assert_eq!(
        item(&conn, Q, "/a.jpg").unwrap().unwrap().state,
        WorkState::Pending
    );
}

#[test]
fn reclaiming_is_available_between_runs_not_only_at_open() {
    let mut conn = mem();
    enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();
    begin(&conn, Q, "/a.jpg").unwrap();
    assert_eq!(reclaim_abandoned(&conn, Q).unwrap(), 1);
    assert_eq!(claimable_paths(&conn), vec!["/a.jpg".to_string()]);
}

/// A row this run no longer owns must not be finishable.
#[test]
fn a_reset_between_claim_and_finish_loses_the_row() {
    let mut conn = mem();
    enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();
    assert!(begin(&conn, Q, "/a.jpg").unwrap());

    reset(&conn, Q).unwrap();
    enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();

    assert!(!set_content_hash(&conn, Q, "/a.jpg", &[3u8; 32]).unwrap());
    assert!(!finish_done(&conn, Q, "/a.jpg", "p", 4, None).unwrap());
    assert!(!finish_failed(&conn, Q, "/a.jpg", 2).unwrap());
    assert!(!finish_skipped(&conn, Q, "/a.jpg", UNSUPPORTED, 1).unwrap());

    let item = item(&conn, Q, "/a.jpg").unwrap().unwrap();
    assert_eq!(item.state, WorkState::Pending);
    assert_eq!(item.retry_count, 0);
    assert_eq!(item.content_hash, None);
}

#[test]
fn a_second_claim_of_a_claimed_row_is_refused() {
    let mut conn = mem();
    enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();
    assert!(begin(&conn, Q, "/a.jpg").unwrap());
    assert!(!begin(&conn, Q, "/a.jpg").unwrap());
}

/// Cancel release only returns rows this run claimed.
#[test]
fn release_only_touches_rows_this_run_claimed() {
    let mut conn = mem();
    enqueue(&mut conn, Q, &["/a.jpg".into(), "/b.jpg".into()]).unwrap();
    begin(&conn, Q, "/a.jpg").unwrap();
    claim_and_finish(&conn, "/b.jpg", "p", 1);
    release(&conn, Q, "/a.jpg").unwrap();
    release(&conn, Q, "/b.jpg").unwrap();
    assert_eq!(
        item(&conn, Q, "/a.jpg").unwrap().unwrap().state,
        WorkState::Pending
    );
    assert_eq!(
        item(&conn, Q, "/b.jpg").unwrap().unwrap().state,
        WorkState::Done
    );
}

#[test]
fn claim_is_from_pending_stale_or_failed() {
    let mut conn = mem();
    enqueue(
        &mut conn,
        Q,
        &[
            "/pending.jpg".into(),
            "/done.jpg".into(),
            "/stale.jpg".into(),
            "/failed.jpg".into(),
            "/dead.jpg".into(),
            "/skipped.jpg".into(),
        ],
    )
    .unwrap();
    claim_and_finish(&conn, "/done.jpg", "pack-1", 1);
    claim_and_finish(&conn, "/stale.jpg", "pack-0", 1);
    mark_stale_for_pack(&conn, Q, "pack-1").unwrap();
    claim_and_fail(&conn, "/failed.jpg", 4);
    for _ in 1..MAX_RETRIES {
        claim_and_fail(&conn, "/dead.jpg", 4);
    }
    claim_and_fail(&conn, "/dead.jpg", 4);
    assert!(begin(&conn, Q, "/skipped.jpg").unwrap());
    assert!(finish_skipped(&conn, Q, "/skipped.jpg", UNSUPPORTED, 1).unwrap());

    let paths = claimable_paths(&conn);
    assert!(paths.contains(&"/pending.jpg".to_string()));
    assert!(paths.contains(&"/stale.jpg".to_string()));
    assert!(paths.contains(&"/failed.jpg".to_string()));
    assert!(!paths.contains(&"/done.jpg".to_string()));
    assert!(!paths.contains(&"/dead.jpg".to_string()));
    assert!(!paths.contains(&"/skipped.jpg".to_string()));
}

#[test]
fn two_queues_in_one_file_are_independent() {
    let mut conn = mem();
    ensure_table(&conn, FACES).unwrap();
    enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();
    enqueue(&mut conn, FACES, &["/a.jpg".into(), "/b.jpg".into()]).unwrap();
    claim_and_finish(&conn, "/a.jpg", "pack-1", 3);

    assert_eq!(stats(&conn, Q).unwrap().done, 1);
    assert_eq!(stats(&conn, Q).unwrap().pending, 0);
    assert_eq!(stats(&conn, FACES).unwrap().pending, 2);
    assert_eq!(stats(&conn, FACES).unwrap().done, 0);

    reset(&conn, Q).unwrap();
    assert_eq!(stats(&conn, Q).unwrap().pending, 0);
    assert_eq!(stats(&conn, FACES).unwrap().pending, 2);
}

#[test]
fn places_work_shares_a_file_with_other_queues() {
    let mut conn = mem();
    ensure_table(&conn, FACES).unwrap();
    ensure_table(&conn, PLACES).unwrap();
    enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();
    enqueue(&mut conn, PLACES, &["/a.jpg".into()]).unwrap();
    claim_and_finish(&conn, "/a.jpg", "pack-1", 3);
    assert!(begin(&conn, PLACES, "/a.jpg").unwrap());
    assert!(finish_done(&conn, PLACES, "/a.jpg", "gazetteer", 1, None).unwrap());

    assert_eq!(stats(&conn, Q).unwrap().done, 1);
    assert_eq!(stats(&conn, PLACES).unwrap().done, 1);
    assert_eq!(stats(&conn, FACES).unwrap().pending, 0);
    reset(&conn, PLACES).unwrap();
    assert_eq!(stats(&conn, Q).unwrap().done, 1);
    assert_eq!(stats(&conn, PLACES).unwrap().pending, 0);
}

#[test]
fn mark_stale_only_demotes_done() {
    let mut conn = mem();
    enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();
    begin(&conn, Q, "/a.jpg").unwrap();
    finish_done(
        &conn,
        Q,
        "/a.jpg",
        "p",
        1,
        Some((4096, Some(1_700_000_000))),
    )
    .unwrap();
    assert_eq!(mark_stale(&conn, Q, "/a.jpg").unwrap(), 1);
    assert_eq!(
        item(&conn, Q, "/a.jpg").unwrap().unwrap().state,
        WorkState::Stale
    );
    assert_eq!(mark_stale(&conn, Q, "/a.jpg").unwrap(), 0);
}

#[test]
fn a_retryable_failure_counts_as_pending_not_failed() {
    let mut conn = mem();
    enqueue(&mut conn, Q, &["/a.jpg".into()]).unwrap();
    claim_and_fail(&conn, "/a.jpg", 2);
    let s = stats(&conn, Q).unwrap();
    assert_eq!(s.pending, 1);
    assert_eq!(s.failed, 0);
}

#[test]
fn enqueue_nfc_and_nfd_share_one_row() {
    let mut conn = mem();
    let nfc = "/lib/caf\u{00E9}.jpg".to_string();
    let nfd = "/lib/cafe\u{0301}.jpg".to_string();
    assert_eq!(enqueue(&mut conn, Q, &[nfd.clone()]).unwrap(), 1);
    assert_eq!(enqueue(&mut conn, Q, &[nfc.clone()]).unwrap(), 0);
    let row = item(&conn, Q, &nfd).unwrap().unwrap();
    assert_eq!(row.path, nfc);
}
