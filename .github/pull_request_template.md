## Summary

Describe the user-visible and architectural effect of this change.

## Verification

List the checks run and any platform or real-input checks that could not run.

## Required human conformance review

Answer every question in prose. If a question does not apply, say why; do not
leave it blank. These are the human-review controls required by ADR 0007 R16.

<!-- r16:shell-policy -->
### Shell-owned policy — ADR 0001 R4 / ADR 0003 R5

Does this change put any ordering, filtering, grouping, formatting, validation, action-availability, or error-classification policy in a shell? Identify the app-core answer or explain why none is affected.

Answer:

<!-- r16:ffi-display-readiness -->
### FFI display readiness — ADR 0003 R6

Does every changed FFI value remain a display-ready primitive, pre-ordered id list, one-slot display record, explicit command DTO, or narrow host-port DTO, with no serialized domain entity crossing? Name the records or DTOs reviewed.

Answer:

<!-- r16:adaptive-native-shell -->
### Adaptive native behavior — ADR 0004 R8–R9

How does each affected shell keep native controls, navigation, accessibility, and host integration while adapting chrome to width, including the 550-CSS-pixel GTK/Comet boundary?

Answer:

<!-- r16:rule-preservation -->
### Rule preservation — ADR 0007 R7–R8

Which domain rules changed, and where are their single app-core assertions and shared preservation fixtures? If no domain rule changed, identify the evidence used to reach that conclusion.

Answer:

<!-- r16:unavailable-real-inputs -->
### Unavailable real inputs — ADR 0007 R9

Did this work require an unavailable real personal export, photo library, contact file, device, or platform input? If so, did the affected work stop and ask for that input instead of synthesizing a plausible substitute?

Answer:
