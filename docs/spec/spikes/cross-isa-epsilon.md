# Spike: Cross-ISA ε

**Status:** answered, 2026-09-11  
**Question:** Same fixture scored on arm64 and x86-64 — maximum absolute score drift, versus the pack's `hysteresis_epsilon`.  
**Outcome:** **ISA causes no significant difference.**

## Answer

Cross-instruction-set score drift is assumed negligible. No arm64 / x86-64 fixture is run, and ε is not sized against one.

ADR 0006 R16 still requires a retention band on every thresholded decision that reaches tier 1 — two devices can still straddle a threshold for reasons other than ISA. What R16 no longer requires is that ε be a measured cross-ISA margin, or that a committed cross-ISA fixture exist.

## Consequences

- R16 is amended: ε is a conventional retention band declared in the pack manifest, not a measured ISA margin.
- The R16 conformance item that demanded a cross-ISA drift fixture is dropped.
- No Phase 2 work item exists to measure ε. M3 is only the SFace model swap.
