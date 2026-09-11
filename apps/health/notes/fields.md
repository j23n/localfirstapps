# FIT field findings

Device: Garmin Instinct 1 Solar.

Garmin has stated that Monitor and Sleep files contain undocumented fields they will not document. `muktihari/fit` preserves unknown messages; the adapter must keep them. Mapping into Apple's type identifiers happens in the projection only.

Record findings here as they are reverse-engineered against real files in `testdata/`. Do not invent gen-2 fields (HRV status, respiration rate, sleep score) — this watch does not produce them.

## Findings

None. The FIT adapter is not implemented.
