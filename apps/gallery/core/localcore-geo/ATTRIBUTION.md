# Place-name data

`localcore-geo` ships a packed gazetteer and admin-0 polygons so photo
coordinates resolve to a locality and a country with no network I/O
(ADR 0006 R10).

## GeoNames

City names, admin divisions, and coordinates come from
[GeoNames](https://www.geonames.org/) `allCountries`, **feature class
`P` only** (populated places, including hamlets). Neighbourhood
sections (`PPLX`) and historical/abandoned codes are dropped.
`admin1CodesASCII` supplies first-order admin names.

Streams, peaks, schools, hotels, and other non-`P` rows are not in the
pack (ADR 0006 R11).

Licence: [Creative Commons Attribution 4.0](https://creativecommons.org/licenses/by/4.0/).

This product includes GeoNames data, © GeoNames contributors.

Rebuild (needs network once; dumps stay in `data/cache/`, gitignored):

```
python3 scripts/pack_geo.py --fetch
```

`--dump cities500` / `--dump cities1000` still work. The committed
artefact is `data/places.bin` (LCG1 v2). Keep it under 256 MB.

## Natural Earth

Country names and admin-0 rings come from
[Natural Earth](https://www.naturalearthdata.com/) `ne_10m_admin_0_countries`
(public domain). Country is point-in-polygon, never the nearest city's
country code.

The 8-city TSV / GeoJSON under `data/subset/` is a debug fixture
(`python3 scripts/pack_geo.py --subset`). It is not what ships.
