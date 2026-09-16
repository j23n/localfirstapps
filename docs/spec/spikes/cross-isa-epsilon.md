# Spike: Cross-ISA ε

**Status:** retention policy chosen; x86-64 fixture recorded, arm64 unavailable, reviewed 2026-09-16
**Question:** Same fixture scored on arm64 and x86-64 — maximum absolute score drift, versus the pack's `hysteresis_epsilon`.  
**Outcome:** **Use conventional hysteresis; cross-ISA adequacy remains a hypothesis.**

## Answer

Phase 5B recorded model hashes, dataset revision, and first-input embedding
hashes on x86-64. No arm64 execution environment was available, so an
identical-model/input cross-instruction-set comparison could not be formed.
The harness rejects reports with different model hashes or dataset revisions;
it does not mistake a current-model run on one ISA and a candidate-model run
on another for cross-ISA evidence. Score drift remains unknown and epsilon is
not sized against it.

ADR 0006 R16 still requires a retention band on every thresholded decision that reaches tier 1 — two devices can still straddle a threshold for reasons other than ISA. What R16 no longer requires is that ε be a measured cross-ISA margin, or that a committed cross-ISA fixture exist.

## Consequences

- R16 is amended: ε is a conventional retention band declared in the pack manifest, not a measured ISA margin.
- The R16 conformance item that demanded a cross-ISA drift fixture is dropped.
- The Phase 5B x86-64 output is recorded in
  `docs/spec/evidence/gallery-phase5b-2026-09-16.json`. Any future claim that
  ε bounds cross-ISA drift requires the matching arm64 report and recorded
  comparison; unavailable hardware is not evidence.
