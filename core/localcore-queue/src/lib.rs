//! Persistent work-queue substrate (ADR 0006 R1–R5).
//!
//! One table shape, many capabilities. A capability names its table and
//! result-count column; this crate owns the SQL and the state machine.
//! The caller opens the SQLite file (schema for results — embeddings,
//! faces, clusters — stays with the app) and passes [`Connection`].
//!
//! # State machine
//!
//! ```text
//! enqueue ──► Pending ──begin──► Hashing ──finish_done────► Done
//!                ▲                  │
//!                │                  ├──finish_failed──► Failed (retryable)
//!                │                  ├──finish_skipped─► Skipped
//!                │                  └──release────────► Pending   (cancel)
//!                │
//!                ├── mark_stale / mark_stale_for_pack: Done ──► Stale
//!                ├── reclaim_abandoned: Hashing ──► Pending     (run start)
//!                └── reopen_skipped: Skipped ──► Pending        (input version)
//! ```
//!
//! Reclaim is an explicit call at **run start**, never implied by opening a
//! connection: two capabilities share one file, and a second opener cannot
//! tell an abandoned row from one the other engine is holding.
//!
//! A capability names its table (`ml_work`, `face_work`, `places_work`, …)
//! and drives the state machine from its own crate. Thumbnails and EXIF
//! are not queued yet.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rusqlite::{params, Connection, OptionalExtension};
use unicode_normalization::UnicodeNormalization;

/// NFC-normalise a path key (ADR 0002 R4). Call before every enqueue
/// and every lookup so NFC and NFD spellings share one row.
pub fn nfc_path(path: &str) -> String {
    path.nfc().collect()
}

/// How many times a row is retried before it counts as permanently failed.
///
/// There is no time-based backoff: runs are user-initiated and the natural
/// spacing between them is the backoff.
pub const MAX_RETRIES: u32 = 3;

/// How long a statement waits for another writer's lock before `SQLITE_BUSY`.
///
/// Five seconds. Every write here is a few small rows — microseconds of
/// lock — so waiting this long means something is wedged.
pub const BUSY_TIMEOUT_MS: u64 = 5_000;

/// One work table. Both fields are compile-time literals interpolated into
/// SQL; nothing user-supplied ever reaches `format!`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Queue {
    /// Table name (`ml_work`, `face_work`, …).
    pub table: &'static str,
    /// Result-count column (`tag_count`, `face_count`, …).
    pub count_col: &'static str,
}

impl Queue {
    /// Name a table and its result-count column.
    ///
    /// Both strings must be SQL identifiers. They are interpolated into
    /// statements; do not pass user input.
    pub const fn new(table: &'static str, count_col: &'static str) -> Self {
        Queue { table, count_col }
    }
}

/// Lifecycle of one item in a work queue.
///
/// Discriminants are persisted, so they are append-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i64)]
pub enum WorkState {
    /// Enqueued, nothing done yet.
    Pending = 0,
    /// Claimed by a worker. A row left here by a kill is reclaimed as
    /// [`WorkState::Pending`] at the start of the next run.
    Hashing = 1,
    /// Processed successfully under `model_pack`.
    Done = 2,
    /// Failed; see `error_code` and `retry_count`.
    Failed = 3,
    /// Was [`WorkState::Done`], but under a pack that is no longer current.
    Stale = 4,
    /// Refused as an unsupported input. Not a failure — re-running will
    /// not help unless the input version (decoder generation) changes.
    Skipped = 5,
}

impl WorkState {
    /// The persisted integer.
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    /// Inverse of [`WorkState::as_i64`]; unknown values read as
    /// [`WorkState::Pending`] so a downgrade re-does work rather than
    /// losing it.
    pub fn from_i64(v: i64) -> WorkState {
        match v {
            1 => WorkState::Hashing,
            2 => WorkState::Done,
            3 => WorkState::Failed,
            4 => WorkState::Stale,
            5 => WorkState::Skipped,
            _ => WorkState::Pending,
        }
    }
}

/// Queue counts, as reported to a UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Rows waiting to be processed, including `stale` and retryable `failed`.
    pub pending: u64,
    /// Rows processed under the current pack.
    pub done: u64,
    /// Rows that failed and are out of retries.
    pub failed: u64,
    /// Rows skipped as an unsupported input.
    pub skipped: u64,
    /// Rows whose result count is non-zero.
    pub tagged: u64,
}

/// The file identity a `done` row was decided against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneRowStat {
    /// Absolute path as enqueued.
    pub path: String,
    /// Size in bytes at the time it was processed.
    pub size: u64,
    /// Last-modified time in whole seconds, when the platform reported one.
    pub modified_unix: Option<i64>,
}

/// One row of a work queue.
///
/// `error_code` is a capability-specific integer. This crate stores and
/// returns it; it does not interpret it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// Absolute path, as handed to [`enqueue`].
    pub path: String,
    /// Content hash of the file bytes, once computed.
    pub content_hash: Option<[u8; 32]>,
    /// Current state.
    pub state: WorkState,
    /// Pack / semantic version that produced the current result.
    pub model_pack: Option<String>,
    /// Last failure classification, capability-defined.
    pub error_code: i64,
    /// How many times this row has failed.
    pub retry_count: u32,
}

/// WAL + `synchronous=NORMAL` + busy timeout. Does not open a file.
///
/// The caller owns the database file and any extra pragmas (`foreign_keys`,
/// …). Two engines sharing one file need WAL so readers and one writer
/// coexist; two writers still collide, and the default is instant
/// `SQLITE_BUSY`.
pub fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    // `journal_mode` returns a row, so it needs `query_row`, not `execute`.
    let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))?;
    Ok(())
}

/// Create the standard work-table shape if it is missing.
///
/// This crate's rustdoc owns that shape. Gallery's `MIGRATIONS` only
/// migrate an existing file onto it — they do not redefine the columns.
/// Used by tests and by a new capability that has no other tables in the file.
pub fn ensure_table(conn: &Connection, q: Queue) -> rusqlite::Result<()> {
    let table = ident(q.table);
    let count = ident(q.count_col);
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {table} (
            path            TEXT PRIMARY KEY,
            content_hash    BLOB,
            state           INTEGER NOT NULL,
            model_pack      TEXT,
            error_code      INTEGER NOT NULL DEFAULT 0,
            retry_count     INTEGER NOT NULL DEFAULT 0,
            {count}         INTEGER NOT NULL DEFAULT 0,
            updated_at      INTEGER NOT NULL,
            file_size       INTEGER,
            file_mtime      INTEGER,
            decoder_version INTEGER
        );
        CREATE INDEX IF NOT EXISTS {table}_state ON {table} (state);
        CREATE INDEX IF NOT EXISTS {table}_hash  ON {table} (content_hash);"
    ))
}

/// Release every `hashing` row back to `pending`.
///
/// A `hashing` row exists because something died holding it. Call at the
/// start of a run, not when opening the file.
pub fn reclaim_abandoned(conn: &Connection, q: Queue) -> rusqlite::Result<usize> {
    let _span = localcore_trace::span("queue", "reclaim_abandoned").extra("table", q.table);
    let n = conn.execute(
        &format!(
            "UPDATE {} SET state = ?1, updated_at = ?2 WHERE state = ?3",
            ident(q.table)
        ),
        params![
            WorkState::Pending.as_i64(),
            now_unix(),
            WorkState::Hashing.as_i64()
        ],
    )?;
    localcore_trace::event("queue", format!("reclaim table={} n={n}", q.table));
    Ok(n)
}

/// Re-open `skipped` rows decided by a different input-version generation.
///
/// ADR 0006 R3: raising the input version re-opens work previously refused
/// as unsupported, and invalidates nothing.
pub fn reopen_skipped(conn: &Connection, q: Queue, current: u32) -> rusqlite::Result<usize> {
    conn.execute(
        &format!(
            "UPDATE {} SET state = ?1, error_code = 0, retry_count = 0, updated_at = ?2
             WHERE state = ?3 AND (decoder_version IS NULL OR decoder_version <> ?4)",
            ident(q.table)
        ),
        params![
            WorkState::Pending.as_i64(),
            now_unix(),
            WorkState::Skipped.as_i64(),
            current as i64
        ],
    )
}

/// Every `done` row that recorded a stat, so a run can spot in-place edits.
pub fn done_rows_with_stat(conn: &Connection, q: Queue) -> rusqlite::Result<Vec<DoneRowStat>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT path, file_size, file_mtime FROM {} WHERE state = ?1
         AND file_size IS NOT NULL",
        ident(q.table)
    ))?;
    let rows = stmt.query_map(params![WorkState::Done.as_i64()], |r| {
        Ok(DoneRowStat {
            path: r.get(0)?,
            size: r.get::<_, i64>(1)?.max(0) as u64,
            modified_unix: r.get(2)?,
        })
    })?;
    rows.collect()
}

/// Demote one `done` row to `stale` because its bytes moved under us.
pub fn mark_stale(conn: &Connection, q: Queue, path: &str) -> rusqlite::Result<usize> {
    conn.execute(
        &format!(
            "UPDATE {} SET state = ?1, updated_at = ?2 WHERE path = ?3 AND state = ?4",
            ident(q.table)
        ),
        params![
            WorkState::Stale.as_i64(),
            now_unix(),
            nfc_path(path),
            WorkState::Done.as_i64()
        ],
    )
}

/// Add `paths` to the queue, idempotently, in one transaction.
///
/// A path already present keeps its state. Returns how many rows were newly
/// inserted.
pub fn enqueue(conn: &mut Connection, q: Queue, paths: &[String]) -> rusqlite::Result<usize> {
    let _span = localcore_trace::span_always("queue", "enqueue")
        .extra("table", q.table)
        .extra("n", paths.len());
    let tx = conn.transaction()?;
    let now = now_unix();
    let mut inserted = 0usize;
    {
        let mut stmt = tx.prepare(&format!(
            "INSERT INTO {} (path, state, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO NOTHING",
            ident(q.table)
        ))?;
        for path in paths {
            inserted += stmt.execute(params![nfc_path(path), WorkState::Pending.as_i64(), now])?;
        }
    }
    tx.commit()?;
    localcore_trace::event(
        "queue",
        format!("enqueued table={} inserted={inserted} asked={}", q.table, paths.len()),
    );
    Ok(inserted)
}

/// Mark every `done` row whose `model_pack` differs from `pack` as stale.
pub fn mark_stale_for_pack(conn: &Connection, q: Queue, pack: &str) -> rusqlite::Result<usize> {
    conn.execute(
        &format!(
            "UPDATE {} SET state = ?1, updated_at = ?2
             WHERE state = ?3 AND (model_pack IS NULL OR model_pack <> ?4)",
            ident(q.table)
        ),
        params![
            WorkState::Stale.as_i64(),
            now_unix(),
            WorkState::Done.as_i64(),
            pack
        ],
    )
}

/// Paths a run should process, oldest-enqueued first.
///
/// Includes `pending`, `stale`, and `failed` rows that have retries left.
/// `limit` of 0 means "everything".
///
/// `root_prefix`, when given, restricts the result to paths under that
/// directory using a literal prefix (`substr`), not `LIKE`/`GLOB`.
pub fn claimable(
    conn: &Connection,
    q: Queue,
    limit: usize,
    root_prefix: Option<&str>,
) -> rusqlite::Result<Vec<WorkItem>> {
    let sql = format!(
        "SELECT path, content_hash, state, model_pack, error_code, retry_count
         FROM {}
         WHERE (state IN (?1, ?2) OR (state = ?3 AND retry_count < ?4))
           AND (?6 IS NULL OR substr(path, 1, length(?6)) = ?6)
         ORDER BY updated_at, path
         LIMIT ?5",
        ident(q.table)
    );
    let _span = localcore_trace::span("queue", "claimable")
        .extra("table", q.table)
        .extra("limit", limit);
    let prefix = root_prefix.map(nfc_path);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![
            WorkState::Pending.as_i64(),
            WorkState::Stale.as_i64(),
            WorkState::Failed.as_i64(),
            MAX_RETRIES,
            if limit == 0 { -1i64 } else { limit as i64 },
            prefix.as_deref(),
        ],
        row_to_item,
    )?;
    let items: Vec<WorkItem> = rows.collect::<rusqlite::Result<_>>()?;
    localcore_trace::event(
        "queue",
        format!("claimable table={} n={}", q.table, items.len()),
    );
    Ok(items)
}

/// One row by path.
pub fn item(conn: &Connection, q: Queue, path: &str) -> rusqlite::Result<Option<WorkItem>> {
    conn.query_row(
        &format!(
            "SELECT path, content_hash, state, model_pack, error_code, retry_count
             FROM {} WHERE path = ?1",
            ident(q.table)
        ),
        params![nfc_path(path)],
        row_to_item,
    )
    .optional()
}

/// Move a row to `hashing`, claiming it for this worker.
///
/// Succeeds only from `pending`, `stale`, or `failed`. Returns whether the
/// claim landed.
pub fn begin(conn: &Connection, q: Queue, path: &str) -> rusqlite::Result<bool> {
    let n = conn.execute(
        &format!(
            "UPDATE {} SET state = ?1, updated_at = ?2
             WHERE path = ?3 AND state IN (?4, ?5, ?6)",
            ident(q.table)
        ),
        params![
            WorkState::Hashing.as_i64(),
            now_unix(),
            nfc_path(path),
            WorkState::Pending.as_i64(),
            WorkState::Stale.as_i64(),
            WorkState::Failed.as_i64()
        ],
    )?;
    Ok(n > 0)
}

/// Record the content hash of a row this run still holds.
pub fn set_content_hash(
    conn: &Connection,
    q: Queue,
    path: &str,
    hash: &[u8; 32],
) -> rusqlite::Result<bool> {
    let n = conn.execute(
        &format!(
            "UPDATE {} SET content_hash = ?1, updated_at = ?2 WHERE path = ?3 AND state = ?4",
            ident(q.table)
        ),
        params![
            hash.as_slice(),
            now_unix(),
            nfc_path(path),
            WorkState::Hashing.as_i64()
        ],
    )?;
    Ok(n > 0)
}

/// Mark a row done under `pack` with `count` results, clearing any previous
/// failure. Only a row this run still holds (`hashing`) is written.
pub fn finish_done(
    conn: &Connection,
    q: Queue,
    path: &str,
    pack: &str,
    count: usize,
    stat: Option<(u64, Option<i64>)>,
) -> rusqlite::Result<bool> {
    let n = conn.execute(
        &format!(
            "UPDATE {}
             SET state = ?1, model_pack = ?2, {} = ?3, error_code = 0,
                 retry_count = 0, updated_at = ?4, file_size = ?6, file_mtime = ?7
             WHERE path = ?5 AND state = ?8",
            ident(q.table),
            ident(q.count_col)
        ),
        params![
            WorkState::Done.as_i64(),
            pack,
            count as i64,
            now_unix(),
            nfc_path(path),
            stat.map(|(size, _)| size as i64),
            stat.and_then(|(_, mtime)| mtime),
            WorkState::Hashing.as_i64()
        ],
    )?;
    Ok(n > 0)
}

/// Mark a row failed, incrementing its retry counter. Requires `hashing`.
pub fn finish_failed(
    conn: &Connection,
    q: Queue,
    path: &str,
    error_code: i64,
) -> rusqlite::Result<bool> {
    let n = conn.execute(
        &format!(
            "UPDATE {}
             SET state = ?1, error_code = ?2, retry_count = retry_count + 1, updated_at = ?3
             WHERE path = ?4 AND state = ?5",
            ident(q.table)
        ),
        params![
            WorkState::Failed.as_i64(),
            error_code,
            now_unix(),
            nfc_path(path),
            WorkState::Hashing.as_i64()
        ],
    )?;
    Ok(n > 0)
}

/// Mark a row skipped, stamping the input-version generation that said so.
/// Requires `hashing`.
pub fn finish_skipped(
    conn: &Connection,
    q: Queue,
    path: &str,
    error_code: i64,
    decoder_version: u32,
) -> rusqlite::Result<bool> {
    let n = conn.execute(
        &format!(
            "UPDATE {} SET state = ?1, error_code = ?2, updated_at = ?3,
                 decoder_version = ?5
             WHERE path = ?4 AND state = ?6",
            ident(q.table)
        ),
        params![
            WorkState::Skipped.as_i64(),
            error_code,
            now_unix(),
            nfc_path(path),
            decoder_version as i64,
            WorkState::Hashing.as_i64()
        ],
    )?;
    Ok(n > 0)
}

/// Release a claimed row back to `pending` without counting a failure.
/// Used when a run is cancelled with work in flight.
pub fn release(conn: &Connection, q: Queue, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        &format!(
            "UPDATE {} SET state = ?1, updated_at = ?2 WHERE path = ?3 AND state = ?4",
            ident(q.table)
        ),
        params![
            WorkState::Pending.as_i64(),
            now_unix(),
            nfc_path(path),
            WorkState::Hashing.as_i64()
        ],
    )?;
    Ok(())
}

/// Queue counts. A `failed` row with retries left is counted as pending.
pub fn stats(conn: &Connection, q: Queue) -> rusqlite::Result<Stats> {
    let _span = localcore_trace::span("queue", "stats").extra("table", q.table);
    let mut stats = Stats::default();
    let mut stmt = conn.prepare(&format!(
        "SELECT state, COUNT(*), SUM({} > 0) FROM {} GROUP BY state",
        ident(q.count_col),
        ident(q.table)
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok((
            WorkState::from_i64(r.get(0)?),
            r.get::<_, i64>(1)? as u64,
            r.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
        ))
    })?;
    for row in rows {
        let (state, count, tagged) = row?;
        match state {
            WorkState::Pending | WorkState::Hashing | WorkState::Stale => stats.pending += count,
            WorkState::Done => {
                stats.done += count;
                stats.tagged += tagged;
            }
            WorkState::Failed => stats.failed += count,
            WorkState::Skipped => stats.skipped += count,
        }
    }
    let retryable: u64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM {} WHERE state = ?1 AND retry_count < ?2",
            ident(q.table)
        ),
        params![WorkState::Failed.as_i64(), MAX_RETRIES],
        |r| r.get::<_, i64>(0).map(|v| v as u64),
    )?;
    stats.pending += retryable;
    stats.failed -= retryable.min(stats.failed);
    localcore_trace::event(
        "queue",
        format!(
            "stats table={} pending={} done={} failed={} skipped={}",
            q.table, stats.pending, stats.done, stats.failed, stats.skipped
        ),
    );
    Ok(stats)
}

/// Drop every queue row. Result caches in other tables are untouched.
pub fn reset(conn: &Connection, q: Queue) -> rusqlite::Result<()> {
    conn.execute(&format!("DELETE FROM {}", ident(q.table)), [])?;
    Ok(())
}

/// Interpolated SQL identifiers must be compile-time literals. Reject
/// anything that is not `[A-Za-z_][A-Za-z0-9_]*` so a mistaken call cannot
/// change the statement shape.
fn ident(name: &'static str) -> &'static str {
    let valid = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    debug_assert!(valid, "queue identifier must be a SQL ident, got {name:?}");
    if !valid {
        // Release builds: refuse to interpolate rather than run attacker SQL.
        // A bad &'static str is a programming error; empty is a safe no-op
        // table name SQLite will reject with "syntax error" / "no such table".
        panic!("localcore-queue: {name:?} is not a SQL identifier");
    }
    name
}

fn row_to_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<WorkItem> {
    let hash: Option<Vec<u8>> = r.get(1)?;
    Ok(WorkItem {
        path: r.get(0)?,
        content_hash: hash.and_then(|h| <[u8; 32]>::try_from(h.as_slice()).ok()),
        state: WorkState::from_i64(r.get(2)?),
        model_pack: r.get(3)?,
        error_code: r.get(4)?,
        retry_count: r.get::<_, i64>(5)?.max(0) as u32,
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
