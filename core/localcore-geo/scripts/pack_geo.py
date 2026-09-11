#!/usr/bin/env python3
"""Pack a gazetteer + admin-0 polygons for localcore-geo.

ADR 0006 R10: country is point-in-polygon; locality is nearest city.
This script is the data-build step. It does not fetch anything.

Inputs (either the committed subset or a later full dump):

    cities — GeoNames cities1000 / cities500 (tab-separated, 19 fields)
             or a subset TSV with a header row:
                 name  admin  lat  lon

    admin0 — GeoJSON FeatureCollection (Natural Earth admin-0, or the
             committed subset). Each feature needs a Polygon or
             MultiPolygon and properties:
                 iso_a2 / ISO_A2 / ADM0_A2
                 name / NAME / NAME_EN / ADMIN

Usage (from this crate root):

    python3 scripts/pack_geo.py
    python3 scripts/pack_geo.py \\
        --cities /path/to/cities1000.txt \\
        --admin0 /path/to/ne_10m_admin_0_countries.geojson \\
        --out data/places.bin

GeoNames is CC BY 4.0 — see ATTRIBUTION.md. Natural Earth admin-0 is
public domain. The committed subset is a hand-traced extract sufficient
for Paris locality lookup and a Niagara (US/CA) border test; it is not
a substitute for the full dumps.
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CRATE = HERE.parent
DEFAULT_CITIES = CRATE / "data" / "subset" / "cities.tsv"
DEFAULT_ADMIN0 = CRATE / "data" / "subset" / "admin0.geojson"
DEFAULT_OUT = CRATE / "data" / "places.bin"

MAGIC = b"LCG1"
VERSION = 1
NO_ADMIN = 0xFFFF


def _prop(props: dict, *keys: str) -> str | None:
    for key in keys:
        value = props.get(key)
        if value is None:
            continue
        text = str(value).strip()
        if text and text not in {"-99", "-1", "None"}:
            return text
    return None


def load_cities(path: Path) -> list[tuple[str, str | None, float, float]]:
    text = path.read_text(encoding="utf-8")
    rows: list[tuple[str, str | None, float, float]] = []
    lines = [ln for ln in text.splitlines() if ln.strip() and not ln.startswith("#")]
    if not lines:
        return rows
    first = lines[0].split("\t")
    # GeoNames cities1000: 19 tab-separated fields, field 0 is a geoname id.
    geonames = len(first) >= 19 and first[0].isdigit()
    start = 0
    if not geonames and first[0].lower() in {"name", "locality"}:
        start = 1
    for ln in lines[start:]:
        parts = ln.split("\t")
        if geonames:
            name = parts[1].strip()
            admin = parts[10].strip() or None
            lat = float(parts[4])
            lon = float(parts[5])
        else:
            if len(parts) < 4:
                raise ValueError(f"{path}: expected name, admin, lat, lon: {ln!r}")
            name = parts[0].strip()
            admin = parts[1].strip() or None
            lat = float(parts[2])
            lon = float(parts[3])
        if not name:
            continue
        rows.append((name, admin, lat, lon))
    return rows


def _rings_from_geom(geom: dict) -> list[list[tuple[float, float]]]:
    kind = geom.get("type")
    coords = geom.get("coordinates")
    if kind == "Polygon":
        polys = [coords]
    elif kind == "MultiPolygon":
        polys = coords
    else:
        return []
    rings: list[list[tuple[float, float]]] = []
    for poly in polys:
        if not poly:
            continue
        # Exterior only. Holes are omitted at this scale; NE ingest can
        # grow this later without changing the on-disk city records.
        ring = [(float(p[0]), float(p[1])) for p in poly[0]]
        if len(ring) >= 2 and ring[0] == ring[-1]:
            ring = ring[:-1]
        if len(ring) >= 3:
            rings.append(ring)
    return rings


def load_admin0(path: Path) -> list[tuple[str, str, list[list[tuple[float, float]]]]]:
    doc = json.loads(path.read_text(encoding="utf-8"))
    features = doc["features"] if doc.get("type") == "FeatureCollection" else [doc]
    out: list[tuple[str, str, list[list[tuple[float, float]]]]] = []
    for feat in features:
        props = feat.get("properties") or {}
        code = _prop(props, "iso_a2", "ISO_A2", "ADM0_A2", "iso2")
        name = _prop(props, "name_en", "NAME_EN", "name", "NAME", "ADMIN")
        if not code or not name or len(code) != 2:
            continue
        rings = _rings_from_geom(feat.get("geometry") or {})
        if not rings:
            continue
        out.append((name, code.upper(), rings))
    return out


def intern(table: list[str], index: dict[str, int], s: str) -> int:
    if s in index:
        return index[s]
    idx = len(table)
    table.append(s)
    index[s] = idx
    return idx


def pack(
    cities: list[tuple[str, str | None, float, float]],
    countries: list[tuple[str, str, list[list[tuple[float, float]]]]],
) -> bytes:
    strings: list[str] = []
    index: dict[str, int] = {}
    buf = bytearray()
    buf.extend(MAGIC)
    buf.extend(struct.pack("<HH", VERSION, 0))
    # Placeholders for counts; filled after the body is known.
    counts_at = len(buf)
    buf.extend(struct.pack("<III", 0, 0, 0))

    city_recs: list[tuple[int, int, int, int]] = []
    for name, admin, lat, lon in cities:
        name_i = intern(strings, index, name)
        admin_i = intern(strings, index, admin) if admin else NO_ADMIN
        city_recs.append((int(round(lat * 1e6)), int(round(lon * 1e6)), name_i, admin_i))

    country_blobs = bytearray()
    for name, code, rings in countries:
        name_i = intern(strings, index, name)
        country_blobs.extend(struct.pack("<H2sH", name_i, code.encode("ascii"), len(rings)))
        for ring in rings:
            country_blobs.extend(struct.pack("<H", len(ring)))
            for lon, lat in ring:
                country_blobs.extend(
                    struct.pack("<ii", int(round(lon * 1e6)), int(round(lat * 1e6)))
                )

    strtab = bytearray()
    for s in strings:
        raw = s.encode("utf-8")
        if len(raw) > 0xFFFF:
            raise ValueError(f"string too long: {s!r}")
        strtab.extend(struct.pack("<H", len(raw)))
        strtab.extend(raw)

    struct.pack_into("<III", buf, counts_at, len(strings), len(city_recs), len(countries))
    buf.extend(strtab)
    for lat_e6, lon_e6, name_i, admin_i in city_recs:
        buf.extend(struct.pack("<iiHH", lat_e6, lon_e6, name_i, admin_i))
    buf.extend(country_blobs)
    return bytes(buf)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--cities", type=Path, default=DEFAULT_CITIES)
    parser.add_argument("--admin0", type=Path, default=DEFAULT_ADMIN0)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args(argv)

    cities = load_cities(args.cities)
    countries = load_admin0(args.admin0)
    if not cities:
        print(f"error: no cities in {args.cities}", file=sys.stderr)
        return 2
    if not countries:
        print(f"error: no countries in {args.admin0}", file=sys.stderr)
        return 2
    blob = pack(cities, countries)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(blob)
    print(
        f"wrote {args.out} ({len(blob)} bytes, "
        f"{len(cities)} cities, {len(countries)} countries)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
