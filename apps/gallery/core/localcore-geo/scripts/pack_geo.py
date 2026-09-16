#!/usr/bin/env python3
"""Pack a gazetteer + admin-0 polygons for localcore-geo.

ADR 0006 R10: country is point-in-polygon; locality is nearest city.
Default dump is GeoNames allCountries, restricted to feature class P
(populated places). Streams, peaks, and POI stay out (R11).

    python3 scripts/pack_geo.py --fetch
    python3 scripts/pack_geo.py --dump cities500 --fetch
    python3 scripts/pack_geo.py --subset   # 8-city fixture, debug only

GeoNames is CC BY 4.0 — see ATTRIBUTION.md. Natural Earth admin-0 is
public domain.
"""

from __future__ import annotations

import argparse
import json
import shutil
import struct
import sys
import urllib.request
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
CRATE = HERE.parent
CACHE = CRATE / "data" / "cache"
DEFAULT_OUT = CRATE / "data" / "places.bin"
SUBSET_CITIES = CRATE / "data" / "subset" / "cities.tsv"
SUBSET_ADMIN0 = CRATE / "data" / "subset" / "admin0.geojson"

MAGIC = b"LCG1"
VERSION = 2
NO_ADMIN = 0xFFFF_FFFF
MAX_PACK_BYTES = 256 * 1024 * 1024

GEONAMES_CITIES = {
    "allCountries": "https://download.geonames.org/export/dump/allCountries.zip",
    "cities500": "https://download.geonames.org/export/dump/cities500.zip",
    "cities1000": "https://download.geonames.org/export/dump/cities1000.zip",
}
ADMIN1_URL = "https://download.geonames.org/export/dump/admin1CodesASCII.txt"
NE_ADMIN0 = {
    "10m": "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_10m_admin_0_countries.geojson",
    "50m": "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_50m_admin_0_countries.geojson",
}

UA = "localcore-geo-pack/2 (+https://github.com/localcore)"


def _download(url: str, dest: Path) -> Path:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.is_file() and dest.stat().st_size > 0:
        print(f"reusing {dest}")
        return dest
    print(f"fetching {url}")
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    tmp = dest.with_suffix(dest.suffix + ".part")
    with urllib.request.urlopen(req, timeout=900) as resp, tmp.open("wb") as out:
        shutil.copyfileobj(resp, out, length=1024 * 1024)
    tmp.replace(dest)
    print(f"wrote {dest} ({dest.stat().st_size} bytes)")
    return dest


def fetch_cities(kind: str) -> Path:
    url = GEONAMES_CITIES[kind]
    zpath = CACHE / f"{kind}.zip"
    _download(url, zpath)
    inner = f"{kind}.txt"
    out = CACHE / inner
    if not out.is_file():
        with zipfile.ZipFile(zpath) as zf:
            name = next(
                n
                for n in zf.namelist()
                if n.rsplit("/", 1)[-1] == inner or n.endswith(".txt")
            )
            print(f"extracting {name}")
            with zf.open(name) as src, out.open("wb") as dst:
                shutil.copyfileobj(src, dst, length=1024 * 1024)
    return out


def fetch_admin1() -> Path:
    return _download(ADMIN1_URL, CACHE / "admin1CodesASCII.txt")


def fetch_admin0(scale: str) -> Path:
    return _download(NE_ADMIN0[scale], CACHE / f"ne_{scale}_admin_0_countries.geojson")


def _prop(props: dict, *keys: str) -> str | None:
    for key in keys:
        value = props.get(key)
        if value is None:
            continue
        text = str(value).strip()
        if text and text not in {"-99", "-1", "None"}:
            return text
    return None


def load_admin1(path: Path) -> dict[str, str]:
    mapping: dict[str, str] = {}
    for ln in path.read_text(encoding="utf-8").splitlines():
        if not ln.strip() or ln.startswith("#"):
            continue
        parts = ln.split("\t")
        if len(parts) >= 2 and parts[0] and parts[1].strip():
            mapping[parts[0]] = parts[1].strip()
    return mapping


# Packed with each city. Lower is more important. Lookup snaps to
# PPLC/PPLA within 25 km so an arrondissement does not beat Paris.
RANK = {
    "PPLC": 0,
    "PPLA": 1,
    "PPLA2": 2,
    "PPLA3": 3,
    "PPLA4": 4,
    "PPLA5": 4,
}
SKIP_FEATURE = {"PPLX", "PPLQ", "PPLH", "PPLW", "PPLCH"}
DEFAULT_RANK = 5


def load_cities(
    path: Path, admin1: dict[str, str] | None = None
) -> list[tuple[str, str | None, float, float, int]]:
    rows: list[tuple[str, str | None, float, float, int]] = []
    admin1 = admin1 or {}
    geonames: bool | None = None
    skipped_header = False
    with path.open(encoding="utf-8") as fh:
        for ln in fh:
            if not ln.strip() or ln.startswith("#"):
                continue
            parts = ln.rstrip("\n").split("\t")
            if geonames is None:
                geonames = len(parts) >= 19 and parts[0].isdigit()
                if not geonames and parts[0].lower() in {"name", "locality"}:
                    skipped_header = True
                    continue
            if skipped_header:
                skipped_header = False
            if geonames:
                if parts[6].strip() != "P":
                    continue
                name = parts[1].strip()
                lat = float(parts[4])
                lon = float(parts[5])
                fcode = parts[7].strip()
                if fcode in SKIP_FEATURE:
                    continue
                cc = parts[8].strip()
                admin_code = parts[10].strip()
                admin = admin1.get(f"{cc}.{admin_code}") if cc and admin_code else None
                rank = RANK.get(fcode, DEFAULT_RANK)
            else:
                if len(parts) < 4:
                    raise ValueError(f"{path}: expected name, admin, lat, lon: {ln!r}")
                name = parts[0].strip()
                admin = parts[1].strip() or None
                lat = float(parts[2])
                lon = float(parts[3])
                rank = DEFAULT_RANK
            if not name or not (-90.0 <= lat <= 90.0) or not (-180.0 <= lon <= 180.0):
                continue
            rows.append((name, admin, lat, lon, rank))
            if len(rows) % 500_000 == 0:
                print(f"  … {len(rows)} populated places")
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
        # Exterior only. Holes are omitted at this scale.
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
        code = _prop(
            props,
            "ISO_A2_EH",
            "iso_a2_eh",
            "iso_a2",
            "ISO_A2",
            "ADM0_A2",
            "iso2",
        )
        name = _prop(props, "NAME_EN", "name_en", "NAME", "name", "ADMIN")
        if not code or not name or len(code) != 2:
            continue
        rings = _rings_from_geom(feat.get("geometry") or {})
        if not rings:
            continue
        out.append((name, code.upper(), rings))
    return out


def _perp_dist(
    p: tuple[float, float], a: tuple[float, float], b: tuple[float, float]
) -> float:
    ax, ay = a
    bx, by = b
    px, py = p
    dx, dy = bx - ax, by - ay
    if dx == 0 and dy == 0:
        return ((px - ax) ** 2 + (py - ay) ** 2) ** 0.5
    t = ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)
    t = max(0.0, min(1.0, t))
    qx, qy = ax + t * dx, ay + t * dy
    return ((px - qx) ** 2 + (py - qy) ** 2) ** 0.5


def rdp(pts: list[tuple[float, float]], eps: float) -> list[tuple[float, float]]:
    if eps <= 0 or len(pts) < 3:
        return pts
    dmax = 0.0
    idx = 0
    for i in range(1, len(pts) - 1):
        d = _perp_dist(pts[i], pts[0], pts[-1])
        if d > dmax:
            idx, dmax = i, d
    if dmax > eps:
        left = rdp(pts[: idx + 1], eps)
        right = rdp(pts[idx:], eps)
        return left[:-1] + right
    return [pts[0], pts[-1]]


def simplify_countries(
    countries: list[tuple[str, str, list[list[tuple[float, float]]]]],
    eps: float,
) -> list[tuple[str, str, list[list[tuple[float, float]]]]]:
    out = []
    for name, code, rings in countries:
        simplified = []
        for ring in rings:
            thin = rdp(ring, eps)
            if len(thin) >= 3:
                simplified.append(thin)
        # Tiny island states can vanish at a coarse ε; keep the original.
        if not simplified:
            simplified = [ring for ring in rings if len(ring) >= 3]
        if simplified:
            out.append((name, code, simplified))
    return out


def intern(table: list[str], index: dict[str, int], s: str) -> int:
    if s in index:
        return index[s]
    idx = len(table)
    table.append(s)
    index[s] = idx
    return idx


def pack(
    cities: list[tuple[str, str | None, float, float, int]],
    countries: list[tuple[str, str, list[list[tuple[float, float]]]]],
) -> bytes:
    strings: list[str] = []
    index: dict[str, int] = {}
    buf = bytearray()
    buf.extend(MAGIC)
    buf.extend(struct.pack("<HH", VERSION, 0))
    counts_at = len(buf)
    buf.extend(struct.pack("<III", 0, 0, 0))

    city_recs: list[tuple[int, int, int, int, int]] = []
    for name, admin, lat, lon, rank in cities:
        name_i = intern(strings, index, name)
        admin_i = intern(strings, index, admin) if admin else NO_ADMIN
        city_recs.append(
            (int(round(lat * 1e6)), int(round(lon * 1e6)), name_i, admin_i, rank)
        )

    country_blobs = bytearray()
    for name, code, rings in countries:
        if len(rings) > 0xFFFF:
            rings = rings[:0xFFFF]
        name_i = intern(strings, index, name)
        country_blobs.extend(struct.pack("<I2sH", name_i, code.encode("ascii"), len(rings)))
        for ring in rings:
            country_blobs.extend(struct.pack("<I", len(ring)))
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
    for lat_e6, lon_e6, name_i, admin_i, rank in city_recs:
        buf.extend(struct.pack("<iiIIB", lat_e6, lon_e6, name_i, admin_i, rank))
    buf.extend(country_blobs)
    return bytes(buf)


def write_pack(
    cities: list[tuple[str, str | None, float, float, int]],
    countries: list[tuple[str, str, list[list[tuple[float, float]]]]],
    out: Path,
) -> bytes:
    blob = pack(cities, countries)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(blob)
    print(
        f"wrote {out} ({len(blob)} bytes, "
        f"{len(cities)} cities, {len(countries)} countries)"
    )
    return blob


def pack_under_budget(
    cities: list[tuple[str, str | None, float, float, int]],
    admin0_10m: Path,
    admin0_50m: Path,
    out: Path,
) -> int:
    for label, path, eps_list in (
        ("NE 10m", admin0_10m, (0.0, 0.001, 0.002, 0.003, 0.005, 0.01)),
        ("NE 50m", admin0_50m, (0.0, 0.01)),
    ):
        countries = load_admin0(path)
        if not countries:
            print(f"warning: no countries in {path}", file=sys.stderr)
            continue
        for eps in eps_list:
            rings = simplify_countries(countries, eps) if eps else countries
            blob = pack(cities, rings)
            note = f"{label}" + (f" rdp={eps}" if eps else "")
            print(f"  candidate {note}: {len(blob)} bytes, {len(rings)} countries")
            if len(blob) <= MAX_PACK_BYTES:
                out.parent.mkdir(parents=True, exist_ok=True)
                out.write_bytes(blob)
                print(
                    f"wrote {out} ({len(blob)} bytes, "
                    f"{len(cities)} cities, {len(rings)} countries, {note})"
                )
                return 0
    print(
        f"error: could not fit cities + admin-0 under {MAX_PACK_BYTES} bytes",
        file=sys.stderr,
    )
    return 2


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--fetch", action="store_true", help="download allCountries + NE admin-0")
    parser.add_argument(
        "--dump",
        choices=sorted(GEONAMES_CITIES),
        default="allCountries",
        help="GeoNames dump when --fetch (default: allCountries)",
    )
    parser.add_argument("--subset", action="store_true", help="pack the 8-city fixture")
    parser.add_argument("--cities", type=Path)
    parser.add_argument("--admin0", type=Path)
    parser.add_argument("--admin1", type=Path)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args(argv)

    if args.subset:
        cities = load_cities(SUBSET_CITIES)
        countries = load_admin0(SUBSET_ADMIN0)
        if not cities or not countries:
            print("error: subset files missing", file=sys.stderr)
            return 2
        write_pack(cities, countries, args.out)
        return 0

    if args.cities and args.admin0:
        admin1 = load_admin1(args.admin1) if args.admin1 else {}
        cities = load_cities(args.cities, admin1)
        countries = load_admin0(args.admin0)
        if not cities:
            print(f"error: no cities in {args.cities}", file=sys.stderr)
            return 2
        if not countries:
            print(f"error: no countries in {args.admin0}", file=sys.stderr)
            return 2
        blob = write_pack(cities, countries, args.out)
        if len(blob) > MAX_PACK_BYTES:
            print(f"error: pack is {len(blob)} bytes (cap {MAX_PACK_BYTES})", file=sys.stderr)
            return 2
        return 0

    if args.fetch or (CACHE / f"{args.dump}.txt").is_file():
        cities_path = fetch_cities(args.dump) if args.fetch else CACHE / f"{args.dump}.txt"
        if args.fetch:
            admin1_path = fetch_admin1()
            admin0_10 = fetch_admin0("10m")
            admin0_50 = fetch_admin0("50m")
        else:
            admin1_path = CACHE / "admin1CodesASCII.txt"
            admin0_10 = CACHE / "ne_10m_admin_0_countries.geojson"
            admin0_50 = CACHE / "ne_50m_admin_0_countries.geojson"
            if args.fetch or not admin1_path.is_file():
                admin1_path = fetch_admin1()
            if not admin0_10.is_file():
                admin0_10 = fetch_admin0("10m")
            if not admin0_50.is_file():
                admin0_50 = fetch_admin0("50m")
        admin1 = load_admin1(admin1_path) if admin1_path.is_file() else {}
        cities = load_cities(cities_path, admin1)
        if not cities:
            print(f"error: no cities in {cities_path}", file=sys.stderr)
            return 2
        print(f"loaded {len(cities)} cities from {cities_path.name} ({len(admin1)} admin1 names)")
        return pack_under_budget(cities, admin0_10, admin0_50, args.out)

    print(
        "error: pass --fetch (allCountries P + NE admin-0), --subset, or --cities/--admin0",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
