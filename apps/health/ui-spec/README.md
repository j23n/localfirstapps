# Health UI reference

This directory is the curated, static Phase 6 reference for localhealth. It
preserves the retired loopback UI's information architecture and data
semantics without preserving an HTTP server, HTML/CSS/JavaScript, Chart.js,
fonts, or rendered-page goldens. It is not a product surface and is not parsed
at runtime.

[`screens.toml`](screens.toml) is the ADR 0004 semantic inventory. R14 reads
it to generate `HealthScreen` identifiers; generated identifiers neither
assemble nor prove a native screen:

```sh
python3 scripts/gen_r14.py
python3 scripts/gen_r14.py --check
```

No native Health shell exists yet. In particular, `chart`, `metric-card`,
`route-map`, and `pause-strip` in `r4_gaps` are names for gaps in ADR 0004's
closed vocabulary, not new screen or item kinds. Adding a chart or metric-card
kind requires an R4 amendment and one native binding per platform. Health also
has no sourced accent or dark palette; none is specified here.

## Screen and legacy-route map

The routes document where the old web UI put each concept. Native shells may
choose platform navigation and must not recreate these URLs merely for parity.

| Screen id | Retired route(s) | Content |
|---|---|---|
| `today` | `/`, `/day` redirect | Selected local day, favourites, workouts, and a gaps banner |
| `medicine` | `/medicine` | Medicine kind catalogue and honest empty state for later lab/medication work |
| `lifestyle` | `/lifestyle` | Lifestyle kind catalogue |
| `sports` | `/sports` | Sports observation and workout-kind catalogue |
| `all-kinds` | `/all`, `/kinds` redirect | Every projected kind, including unclassified Apple kinds |
| `kind-detail` | `/{medicine,lifestyle,sports,all}/{slug}`, `/kind`, `/timeline` redirect | Headline, range/window/source filters, series, recent values, and workout sessions when applicable |
| `workout-session` | `/sports/{slug}/{id}`, `/all/{slug}/{id}` | Route, summary metrics, heart-rate axes, elevation, pace, pauses, splits, and source |
| `gaps` | `/gaps` | Overall/per-kind coverage and links from missing local days back to Today |
| `source-files` | `/blobs` | Imported file metadata only; no preview |

Today alone owns local-day navigation. Domain and All screens are catalogues,
not date-filtered views. Range/window/source controls belong to kind detail.
The source chooser appears only when multiple sources overlap on a local day.

## Data contracts and deterministic fixtures

[`data-contracts.toml`](data-contracts.toml) pins:

- range-to-grain selection and local-calendar bucket boundaries;
- configured count/sum/mean/min/max/latest reductions;
- null empty buckets, from-zero totals, and min–max range bars;
- HealthKit percentage display, headline/baseline behavior, and row limits;
- per-day source preference, overlap filtering, and provenance requirements;
- workout summary, route, heart-rate, elevation, pace, pause, and
  deterministic downsampling behavior.

[`metric-kinds.toml`](metric-kinds.toml) retains the display name, domain, and
aggregate assigned to each known Apple observation/episode kind. It is static
design input; per-archive overrides and runtime parsing were intentionally not
retained.

The fixtures retain the small cases that carried behavior independently of
the old renderer:

- [`fixtures/metric-series.json`](fixtures/metric-series.json) covers empty
  buckets, min–max, totals, HealthKit percentages, and episode grain.
- [`fixtures/source-selection.json`](fixtures/source-selection.json) covers a
  preferred-source collision, an explicit source lock, complementary source
  days, and source record identity.

Rendered HTML goldens were intentionally not retained: they pinned templates,
CSS classes, Chart.js JSON, and nonce normalization rather than a portable UI
contract. The native shells should consume core-owned display records and use
native rendering once the R4 gaps are resolved; they must not port the old Go
handlers or frontend assets.
