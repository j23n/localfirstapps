# LocalContactsTests

Unit + integration tests for LocalContacts. Run via the `LocalContacts`
scheme — the test target is wired into it, and CI executes
`xcodebuild test` before the archive step.

## Conventions

- **Temp dirs per test.** File-system tests build a unique folder under
  `FileManager.default.temporaryDirectory` and remove it via `defer`.
  Never touch the user-selected folder.
- **No `UserDefaults.standard`.** `BookmarkManager` accepts an injected
  `UserDefaults`; tests pass a per-test `UserDefaults(suiteName:)`.
- **Skip `setFolder`.** It goes through bookmarks, security-scoped
  resources, and `UserDefaults`. File-system tests assign
  `store.folderURL` directly and call `loadContacts` / `save` / etc.
- **Skip `CNContactStore` operations.** Anything authorization-gated is
  out of scope for the unit suite — see "Follow-up work" below for the
  protocol-shim plan.
- **Swift Testing** (`@Test`, `#expect`, `#require`) for new tests.

## Current coverage

| Suite | What it covers |
| --- | --- |
| `VCardParserTests` | Round-trip identity, header skipping, name components, type-label extraction (TYPE=, vCard 2.1 bare types, defaults), group prefix vs. semicolon-in-params, addresses, all three BDAY formats + malformed, base64 PHOTO + URI fallback, CATEGORIES, X-LOCALCONTACTS-ID, unknown-field preservation, escape sequences, line folding (CRLF+space and LF+tab), `parseMultiple` ordering and blanks between cards, both `assignDefaultID` branches. |
| `VCardWriterTests` | Header order + CRLF endings, optional-field omission/inclusion, escaping (NOTE and FN), BDAY year/no-year/missing, base64 PHOTO encoding, unknown-field round-trip, filename suggestion (lowercased given-family, sanitization, lcid fallback for empty/punctuation-only), fully-populated end-to-end round trip, FN→displayName fallback. |
| `ContactTests` / `PostalAddressTests` | `displayName` / `initials` / `sortLetter` fallbacks, `age` computation, **`copy()` deep-copies postal addresses**, `formatted` and `isEmpty` semantics. |
| `ContactsStoreTests` | Pure computed properties: `allTags` counting + sorting, search across name / org / jobTitle / phone / email, `selectedTag` and `showConflictsOnly` filters, locale-aware sort, `groupedContacts` ordering, all four `layoutMode` cases. |
| `ContactsStoreFileSystemTests` | Integration against per-test temp folders. ID migration with persistence + reload stability, save in both `.oneFilePerContact` and `.singleFile` layouts, filename-collision suffixing, **multi-vCard sibling preservation** (the regression test for editing one contact in a shared file), delete semantics for both layouts, bulk delete file collapse, `renameTag` with deduplication and `selectedTag` carryover, `deleteTag`, `assignTag` with no-duplicate guarantee + on-disk verification. |
| `BookmarkManagerTests` | save/load round-trip, `hasBookmark` / `clearBookmark`, resilience to corrupt stored data (writes garbage at `BookmarkManager.bookmarkKey` and asserts nil). |
| `CNSyncServiceTests` | `cnLabel` mapping table (case-insensitive, fax-non-phone fallback, unknown-label catch-all), `contactDiffers` for every diff trigger plus the documented "phone label-only ≠ diff" and "photo bytes intentionally ignored" cases. |

## Production-side accommodations

The suite required three small testability tweaks to the app target:

- `BookmarkManager` accepts an injected `UserDefaults` (defaults to
  `.standard`); `bookmarkKey` is `internal static`.
- `CNSyncService.cnLabel` and `contactDiffers` are `nonisolated`. They
  access no actor state, so this is safe — and lets tests call them
  synchronously without crossing an actor boundary (which would trip
  `Sendable` on `CNContact`).
- `VCardParser.parse(...)` / `parseMultiple(...)` take an
  `assignDefaultID: Bool = true` parameter. `ContactsStore.loadContacts`
  passes `false` so missing `X-LOCALCONTACTS-ID` lines surface as empty
  strings and the migration block can assign + persist a stable UUID
  exactly once. Default `true` keeps casual callers safe.

## Follow-up work

Not blockers for the current suite, but worth picking up next:

- **`CNContactStoreProtocol` shim + stateful sync tests.** The current
  `CNSyncServiceTests` only cover pure logic. Extracting a small
  protocol around the `CNContactStore` calls (`groups`,
  `unifiedContacts`, `execute`, `currentHistoryToken`, `containers`)
  would let us test `pushContact`, `fetchChanges`, and
  `fullReconciliation` against a fake — including first-push vs.
  update, externally-deleted detection, externally-added events,
  history-token short-circuit, and reconciliation cleaning up
  duplicate groups.
- **Extract `processChangeEvents` and `applyMerge`.** Both currently
  live inside views (`LocalContactsApp.checkForExternalChanges` and
  `ConflictResolutionSheet.applyMerge`). Pulling them onto
  `ContactsStore` / a merge helper would let us test the
  conflict-state state machine and field-by-field merge selection.
- **One XCUITest happy-path smoke.** Launch with a seeded temp folder
  via a debug-only argument, list → add → search → edit → bulk delete.
- **Coverage gaps the existing suites skip.** Empty `Data()` into
  `parse`; `parseMultiple` on malformed input (BEGIN with no END, junk
  between blocks); a file-system path exercising `.mixed` layout.
- **`BookmarkManager` `Sendable` honesty.** Currently
  `@unchecked Sendable` because it stores a `UserDefaults`. Pass
  `UserDefaults` through each method instead of holding it; restores
  compile-time safety.
