# Apple Health export — field notes

Source: a real Apple Health export (HealthKit Export Version 14, 102 MB, 2026-09-08). Fixtures in `testdata/apple/` are **verbatim slices** of that file with identifying values substituted (date of birth, biological sex, device identifiers, source names, timezone, height / body mass / VO2 max) and GPX coordinates shifted by an undisclosed constant offset (route geometry preserved, location not). Nothing was hand-written.

## Dedup

Key = `hash(type, sourceName, startDate, endDate, value, sorted MetadataEntry pairs)`.

The key is a **group**, not a unique row. On each blob, insert `max(0, count_in_this_export − count_already_in_db)` rows, numbered `n = 1…`.

Records in this export that share type, sourceName, startDate, endDate and value:

| What | Under the key |
|---|---|
| HeartRate 68 at 2025-09-12 20:04:07 +0200, motion context `0` vs `2` | **Two keys** |
| Watch StepCount 198, 2025-09-16 19:45:01 +0200 (identical pair) | One key, group size 2 |
| Watch Distance 0.140247, same window | One key, group size 2 |
| iPhone StepCount 232, 2026-08-24 07:52:47 +0200 | One key, group size 2 |
| iPhone Distance 0.139193, same window | One key, group size 2 |

The identical pairs differ only in `creationDate` / device pointer, which are not in the key.

## source_precedence (explicit, no heuristics)

Defined in `config/sources.toml` (embedded default; per-archive override at `<archive root>/config/sources.toml`).

| sourceName | rank | Why |
|---|---|---|
| Apple Watch | 10 | On-wrist sensors; preferred for HR, HRV, sleep, watch steps |
| iPhone | 20 | The phone (Health `device` name is iPhone); pedometer/GPS when the watch is off |
| Pace | 30 | A third-party app. Keep raw rows; never prefer over Watch or phone |
| Health | 40 | Health app / system entries (rare) |
| Mindful | 50 | Third-party meditation (`MindfulSession`) |
| Sleep | 60 | Third-party sleep app (rare) |

Unlisted names get rank 100. Do not substring-match `"phone"` / `"watch"`.

## Correlation — UNVERIFIED

DTD: “Any Records that appear as children of a correlation also appear as top-level records in this document.”

This export has **zero `<Correlation>` elements**. Nested-record skip is implemented and has **never executed against real data**. Do not treat it as confirmed.

## Medication

Logged in the Health app; **not present in the export**.

- `export.xml`: no `HKMedication*` / dose events / `ClinicalRecord`
- `export_cda.xml`: 73 MB of the same vitals/workouts, no medication / `substanceAdministration`
- `Me.CardioFitnessMedicationsUse` = `None` (characteristic, not a dose log)

Apple’s XML/CDA export omits the Medications feature. Medication history is entered manually as events.

## DTD

HealthKit Export Version 14 **includes** `WorkoutStatistics` on `Workout` and `CardioFitnessMedicationsUse` on `Me`. Parse permissively anyway; do not validate.

## Local calendar days (`archive observations -on`)

Stored timestamps are UTC. `-on`/`-from`/`-to` are **local** dates. Conversion uses `$ARCHIVE_TZ` (IANA or `+0200`), else the process zone. The UTC window and the offset in effect at the start of that local day are printed on stderr.

Fixture check: Watch StepCount `2025-09-12 00:27:33 +0200` is `2025-09-11T22:27:33Z`. `-on 2025-09-12` with offset `+0200` includes it; a naive UTC-day filter on `2025-09-12` would miss it.

## Fixture

One Record of every HK type present (44 types, 19 distinct units), extra Watch/iPhone samples, all five real collision groups, one Running workout with events/statistics/route, 12 ActivitySummaries.

Units seen: (none), `%`, `W`, `cm`, `count`, `count/min`, `dBASPL`, `hr`, `kcal`, `kcal/hr·kg`, `kg`, `km`, `km/hr`, `m`, `m/s`, `mL/min·kg`, `mg`, `min`, `ms`.

## Workout → route link

`Workout` contains `WorkoutRoute` → `FileReference path="/workout-routes/route_YYYY-MM-DD_H.MMam.gpx"`. The path is stored on the episode body (`routes[].path`). The GPX lives in the same zip as `export.xml` (`apple_health_export/workout-routes/…`). Projection opens it via the episode's `blob_sha256`; no new event.

Verified against the fixture Running workout route plus a second export route that contains a GPS gap.

Apple GPX 1.1: one `<trk><trkseg>`, 1 Hz `<trkpt lon lat>` with `<ele>`, `<time>` (UTC `Z`), and extensions `speed`, `course`, `hAcc`, `vAcc`. No HR in the route file.

## WorkoutEvent types (this export)

From the fixture Running workout (verbatim slice of the 2026-09-08 export):

| type | n | notes |
|---|---|---|
| `HKWorkoutEventTypeSegment` | 10 | auto km / distance splits (duration in min). Not a user lap. |
| `HKWorkoutEventTypePause` | 2 | one 4 s mid-run pair; one unpaired at `endDate` (Stop) |
| `HKWorkoutEventTypeResume` | 1 | matches the 4 s pause |

`HKWorkoutEventTypeMotionPaused` / `MotionResumed` and `HKWorkoutEventTypeLap` do **not** appear on this workout. Manual Pause vs auto-pause is distinguishable *when both exist* (different type identifiers). This slice only has Pause/Resume.

Pause threshold default 60 s (`config/projection.toml`). Events are sorted by time before pairing; a second pause while already paused is ignored; a pause still open at `endDate` closes there. The 4 s pause counts toward `pause_count` / paused seconds and does **not** split the route. The unpaired end Pause closes at `endDate` with 0 s and counts toward `pause_count`: the fixture projects `pause_count = 2`, `paused = 4 s`.

`moving_seconds + paused = elapsed_seconds`. Apple's `duration` attribute is moving time (2529 s); wall clock 19:35:34–20:17:47 +0200 is 2533 s elapsed.

## route_polyline binary (ARPL v1)

Little-endian. Readable without Go.

```
offset  size  field
0       4     magic "ARPL"
4       1     version = 1
5       1     flags = 0
6       2     nseg (uint16)
then for each segment:
        2     npts (uint16)
        npts × (lat float32, lon float32)
```

Elevation is **not** in the blob (ascent/descent are columns). Ascent/descent use 3 m hysteresis: a change accumulates only once the altitude has moved ≥ 3 m from the last accepted point, so GPS jitter does not add phantom climb. Missing `<ele>` stays SQL NULL, never 0. No points / no GPX in the zip → all route columns NULL. Segments longer than 65535 points are stored as consecutive chunks. A GPX that fails to open or parse is reported on stderr (`warn: route …`) and leaves the route columns NULL. All `WorkoutRoute` references on a workout are read, in file order.

RDP tolerance 10 m in local metres (equirectangular, `cos(lat0)` on longitude). Segments split on GPS gaps ≥ pause threshold, implausible speed (> 50 m/s), or a WorkoutEvent pause ≥ threshold.

## Simplification (fixture routes)

| file | raw | segments | after RDP |
|---|---|---|---|
| `route_2025-09-14_8.17pm.gpx` (the Running workout) | 2526 | 1 | 32 |
| `route_2025-09-23_gap.gpx` (12 points around a 2919 s GPS gap) | 12 | 2 | — |

The committed export fixture has **1** workout, **1** route, **10** Segment events, **0** Lap events.
