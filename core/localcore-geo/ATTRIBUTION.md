# Place-name data

`localcore-geo` ships a packed gazetteer and admin-0 polygons so photo
coordinates resolve to a locality and a country with no network I/O
(ADR 0006 R10).

## GeoNames

City names, admin divisions, and coordinates in the gazetteer come from
[GeoNames](https://www.geonames.org/) (`cities1000` / `cities500`).

Licence: [Creative Commons Attribution 4.0](https://creativecommons.org/licenses/by/4.0/).

This product includes GeoNames data, © GeoNames contributors.

The committed pack is a **subset** (a handful of cities) so this tree
stays small and builds offline. `scripts/pack_geo.py` ingests a full
GeoNames dump when one is available:

```
python3 scripts/pack_geo.py \
    --cities /path/to/cities1000.txt \
    --admin0 /path/to/ne_10m_admin_0_countries.geojson \
    --out data/places.bin
```

## Natural Earth

Country names and admin-0 rings are intended to come from
[Natural Earth](https://www.naturalearthdata.com/) `admin_0_countries`
(public domain). The committed subset uses hand-simplified rings that
are sufficient for:

- Paris (France) locality lookup
- A Niagara River border test (United States / Canada)

They are not a substitute for the 10 m Natural Earth set. Rebuild from
NE when packing a full gazetteer so every land border is represented.
