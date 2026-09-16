# Spike: Cross-ISA ε

**Status:** retention policy chosen; ISA measurement unanswered, reviewed 2026-09-14
**Question:** Same fixture scored on arm64 and x86-64 — maximum absolute score drift, versus the pack's `hysteresis_epsilon`.  
**Outcome:** **Use conventional hysteresis; cross-ISA adequacy remains a hypothesis.**

## Answer

No arm64 / x86-64 fixture was run, so cross-instruction-set score drift is
unknown and epsilon is not sized against it.

ADR 0006 R16 still requires a retention band on every thresholded decision that reaches tier 1 — two devices can still straddle a threshold for reasons other than ISA. What R16 no longer requires is that ε be a measured cross-ISA margin, or that a committed cross-ISA fixture exist.

## Consequences

- R16 is amended: ε is a conventional retention band declared in the pack manifest, not a measured ISA margin.
- The R16 conformance item that demanded a cross-ISA drift fixture is dropped.
- Phase 2 did not measure ε. Any future claim that it bounds cross-ISA drift
  requires an arm64/x86-64 fixture and recorded result.
