# Test Plan

LocalContacts ships with no automated tests today. This document is a
prioritized plan for adding them. The goal is not 100% coverage — it is to
catch the regressions that would silently corrupt user data or break the
file/Apple-Contacts contract that the whole app rests on.

## Where bugs would hurt most

The codebase has three layers, listed by blast radius if broken:

1. **vCard read/write** (`VCardParser`, `VCardWriter`) — the on-disk format.
   A parser bug drops user data; a writer bug emits invalid vCards that other
   tools can't read. These are pure functions over `String`/`Data`, so they
   are also the cheapest to test.
2. **`ContactsStore` file orchestration** — load, save, delete, tag rename,
   bulk delete. Multi-vCard files mean one save touches sibling contacts;
   getting this wrong silently deletes other people's contacts. Layout mode
   detection drives whether a save creates a new file or appends to a shared
   one.
3. **`CNSyncService` reconciliation** — diff detection, ID mapping,
   add/update/delete events, full-reconciliation "nuke and pave". Bugs here
   produce duplicate Apple Contacts, orphan group memberships, or false
   conflict banners.

The plan below tracks that priority order.

## Tooling and target layout

- **XCTest** for unit and integration tests, paired with **Swift Testing**
  (`@Test` / `#expect`) for new tests — both are first-class in Xcode 16.
- Add two targets to `project.yml`:
  - `LocalContactsTests` (unit + integration, runs on simulator)
  - `LocalContactsUITests` (XCUITest, simulator only, smoke level)
- Wire both schemes into the existing `.github/workflows` so PRs run
  `xcodebuild test -scheme LocalContacts -destination 'platform=iOS Simulator,name=iPhone 16'`
  before the archive step.
- Use a **temporary directory per test** (`FileManager.default.temporaryDirectory`)
  for any file-system test, and tear it down in `tearDown()`. Never touch the
  user's selected folder or `UserDefaults.standard` — inject a suite-scoped
  `UserDefaults(suiteName:)` where needed.
- For `CNSyncService`, extract a small `CNContactStoreProtocol` shim around
  the calls that actually hit the system (`groups`, `unifiedContacts`,
  `execute`, `currentHistoryToken`, `containers`). Tests pass a fake; on-device
  smoke tests still exercise the real `CNContactStore`.

## Unit tests

### `VCardParser` (highest value, lowest cost)

Test data lives in `LocalContactsTests/Fixtures/*.vcf` so real-world quirks
can be added over time. Cases:

- **Round-trip identity** — for every `Contact` produced by the writer, the
  parser must return an equivalent `Contact` (all fields equal except `id`,
  which is a fresh UUID per parse). Drive this with property-based-style
  fixtures: empty contact, name-only, every-field-populated, multi-value
  fields (3 phones, 2 emails, 2 addresses).
- **Line folding** — input with `\r\n ` and `\n\t` continuations must
  reassemble to a single logical line. Verify a 200-char NOTE folded at 75
  chars round-trips byte-identical.
- **Unescape** — `\n`, `\N`, `\,`, `\;`, `\\` in FN/NOTE/CATEGORIES.
- **Group prefix** — `item1.TEL;TYPE=cell:+1...` must be parsed as TEL.
  Negative case: `home;TYPE=foo:bar` (semicolon before colon, no group
  prefix) must NOT be treated as a group.
- **Type label extraction**:
  - `TEL;TYPE=CELL,VOICE,PREF` → label `cell` (skips `voice`/`pref`).
  - `TEL;HOME:` (vCard 2.1 bare type) → label `home`.
  - `EMAIL;TYPE=INTERNET,WORK` → label `work` (skips `internet`).
  - `URL` with no TYPE → default `homepage`.
  - `TEL` with no TYPE → default `mobile`.
- **Birthday formats** — `19850314`, `1985-03-14`, `--03-14` (no year),
  malformed `1985-13-99` (returns nil, doesn't crash), empty.
- **Photo** — `PHOTO;ENCODING=b;TYPE=JPEG:<base64>` decodes to non-nil Data.
  `PHOTO;VALUE=URI:https://...` is preserved as an unknown field, not
  parsed into `photoData`.
- **`X-LOCALCONTACTS-ID`** — preserved exactly; missing → migration path
  in `loadContacts` covered separately.
- **Unknown fields** — `X-CUSTOM-FIELD:weird` survives a parse → write
  cycle verbatim. Same for `IMPP`, `RELATED`, `ANNIVERSARY`, etc.
- **Malformed input** — empty `Data`, non-UTF-8 bytes, missing
  `BEGIN:VCARD`, missing `END:VCARD`, only `BEGIN:VCARD\nEND:VCARD` →
  must not crash; returns `nil` or empty array as appropriate.
- **`parseMultiple`** — file with three `BEGIN:VCARD ... END:VCARD` blocks
  yields three contacts; preserves order; survives extra blank lines and
  text between cards.

### `VCardWriter`

- Every field round-trips through the parser (see above).
- **Escaping** — strings containing `,`, `;`, `\`, and `\n` are escaped in
  output and unescape correctly on read. NOTE with embedded newline is the
  canary case.
- **Filename suggestion** — `suggestedFileName(for:)`:
  - "Anna Müller" → `anna-mller.vcf` (sanitizer strips non-`[a-z0-9-]`).
  - Empty given/family → `<localContactsID>.vcf`.
  - Punctuation-only ("!!!") → `<localContactsID>.vcf`.
- **Required header** — output always contains `BEGIN:VCARD`, `VERSION:3.0`,
  `END:VCARD`, `X-LOCALCONTACTS-ID:` in that order, with `\r\n` line endings.
- **Empty optional fields** — ORG/TITLE/NICKNAME/NOTE/CATEGORIES omitted
  when empty (don't write `ORG:`).
- **Birthday with/without year** — `BDAY:1985-03-14` vs `BDAY:--03-14`.
- **Photo** — base64 wrapped at 76 chars; `ENCODING=b;TYPE=JPEG`.

### `Contact` and `PostalAddress`

- `displayName` — falls back from `fullName` → joined parts → `"No Name"`.
- `initials` — `"AB"`, `"A"`, `"?"` for empty.
- `sortLetter` — letters return uppercased first letter; numerics, emoji,
  empty all return `"#"`.
- `age` — returns nil if year missing; computes correctly for a known
  birthday (use a fixed `Calendar` and reference date — not `Date()` —
  via dependency injection or a wrapper to keep the test deterministic).
- `copy()` — produces a value-equivalent `Contact` whose `postalAddresses`
  array contains a *new* `PostalAddress` instance (mutating the copy must
  not affect the original; this catches the address `.copy()` regression).
- `PostalAddress.isEmpty` — true for default-init; false if any field set.
- `PostalAddress.formatted` — joins non-empty fields with newlines, joins
  state+postalCode with a space, omits empty lines entirely.

### `ContactsStore` — pure computed properties

These don't need a folder; build the store in-memory.

- `allTags` — sums tag counts across contacts, sorted alphabetically; an
  empty list returns `[]`; tags appearing twice on one contact count once
  per occurrence on different contacts.
- `filteredContacts`:
  - search matches displayName / org / jobTitle / phone / email,
    case-insensitive; partial substring; phone match preserves digits.
  - `selectedTag` filters by category; `showConflictsOnly` overrides
    `selectedTag`.
  - empty `searchText` returns all (alphabetically sorted by displayName,
    locale-aware case-insensitive — verify "ábel" sorts near "abel").
- `groupedContacts` — groups by `sortLetter`, sorted A→Z then `#`.
- `hasConflicts` — true iff any contact has non-nil `conflictState`.
- `layoutMode`:
  - `[]` → `.empty`.
  - one contact in `a.vcf` → `.oneFilePerContact` (per the docstring's
    explicit rule).
  - 5 contacts each in their own file → `.oneFilePerContact`.
  - 3 contacts all in `bundle.vcf` → `.singleFile("bundle.vcf")`.
  - 2 in `bundle.vcf` + 1 in `solo.vcf` → `.mixed`.

### `BookmarkManager`

- Use a custom `UserDefaults(suiteName:)` (inject if needed; otherwise
  test against a fixture suite name and clean up in `tearDown`).
- `saveBookmark` then `loadBookmark` returns the same URL.
- `clearBookmark` makes `hasBookmark` false and `loadBookmark` return nil.
- Stale bookmark path is harder to simulate in unit tests; gate that as
  an integration test or skip.

## Integration tests (file system)

These build a real `ContactsStore`, point it at a `tmp` folder, drop
fixture `.vcf` files in, and assert in-memory + on-disk state.

- **Load + parse + sort** — drop 5 fixture files, call `loadContacts()`,
  expect 5 contacts, sorted, `lastSyncedAt` populated.
- **ID migration** — fixture file with no `X-LOCALCONTACTS-ID` is loaded;
  contact gets a fresh UUID; the file on disk is rewritten to include the
  ID; reloading does not generate a *new* UUID. This guards the comment
  in `ContactsStore.swift:155` warning that ID drift breaks Apple-Contacts
  links.
- **Save in `.oneFilePerContact` layout** — saving a new contact creates
  a file named per `suggestedFileName`; collision appends `-1`, `-2`.
- **Save in `.singleFile` layout** — folder has `everyone.vcf` with two
  contacts; saving a third contact writes back to `everyone.vcf` (no new
  file created); on-disk file contains 3 `BEGIN:VCARD` blocks in
  predictable order.
- **Edit one contact in a multi-vCard file** — file has A, B, C; user
  edits B; file still contains A, B', C and no others. This is the
  highest-value regression test in the suite — it's what would silently
  delete other contacts.
- **Delete from multi-vCard file** — file has A, B, C; delete B;
  file still has A, C; file is *not* removed. Delete the last contact
  in a file → file is removed.
- **`renameTag` / `deleteTag`** — fixture has tag "friends" on contacts
  in two different files; rename → both files rewritten on disk; only
  those files (untouched files have unchanged mtime — assert via
  `attributesOfItem`).
- **`renameTag` deduplication** — contact has tags `["a", "b"]`;
  rename `"a"` → `"b"` → contact ends with `["b"]`, not `["b", "b"]`.
- **`deleteMultiple`** — selects 3 contacts spread across 2 files;
  resulting files contain only the survivors; CN delete called once
  per contact (via fake sync service).
- **`uniqueFileName` collision** — folder already has `john-doe.vcf` on
  disk that is *not yet loaded*; saving a new "John Doe" must produce
  `john-doe-1.vcf`, not overwrite the existing file. This guards both
  the in-memory and on-disk collision branches.

## `CNSyncService` tests

Pure logic first — these don't need entitlements:

- **`contactDiffers`** — for each diff trigger (givenName, phone set,
  email set, address tuple, organization, etc.), assert true; on equal
  contacts, false. Photo intentionally ignored — assert that two
  contacts identical except for `imageData` return false.
- **`cnLabel`** — exhaustive table: each of `home`, `work`, `mobile`,
  `cell`, `main`, `iphone`, `fax` (phone vs. non-phone), `pager`,
  `other`, and an unknown label maps to the documented `CNLabel*`
  constant.
- **`mapContactToCN` / `extractCNContactData`** — round-trip a
  populated `Contact` through `CNMutableContact` (no save) and back
  via `extractCNContactData`. All fields equal.

Stateful logic via the `CNContactStoreProtocol` fake:

- **First push** — `pushContact` on a contact with no mapping → fake
  records `add` + `addMember`; mapping is persisted; second push for
  the same `localContactsID` records `update`, not a second `add`.
- **`fetchChanges` delta**:
  - mapped contact's CN counterpart deleted from group → `.deleted` event.
  - mapped contact's CN counterpart edited → `.updated` event with diffed
    payload.
  - unmapped CN contact present in group → `.added` event with empty
    `localContactsID`.
  - history token unchanged since last call → returns `[]` immediately
    (covers the early-return shortcut).
- **`fullReconciliation`** — fake has 2 stale contacts in the
  LocalContacts group + 1 duplicate group; reconciliation deletes both
  contacts and the duplicate group, creates a fresh group, and adds N
  new contacts; mapping is rebuilt from the reconciled identifiers.
- **`claimCNContact`** — sets mapping without creating a duplicate CN
  contact. Critical for the auto-import path (`importExternalContact`
  in `LocalContactsApp.swift`).

## App-level integration

`LocalContactsApp.checkForExternalChanges` is the entry point users
trigger by foregrounding the app. Move its body into a testable function
on `ContactsStore` (e.g. `processChangeEvents`) and verify:

- `.updated` event with a matching local contact whose `conflictState`
  is nil → sets `.externalEdit(data)`.
- `.updated` event with a matching contact already in conflict → leaves
  state untouched (no clobber).
- `.deleted` event for an existing contact → sets `.externalDelete`.
- `.added` event → calls `save` then `claimCNContact` (no duplicate
  push).

`ConflictResolutionSheet.applyMerge` is also testable as pure logic if
you extract `mergeFields(selections:)` off the View. Tests:

- All-local selection → contact unchanged.
- All-Apple selection → every diffed field replaced from `externalData`;
  fields not in `diffs` (because identical) are not touched.
- Mixed selection → only chosen fields change.
- Photo merge — local nil + Apple non-nil ⇒ Apple wins; local non-nil ⇒
  local wins regardless of Apple.

## UI / smoke (XCUITest)

Keep this thin. The View layer mostly delegates to the store, and the
store will already be covered. One end-to-end happy-path test:

1. Launch app with `-LCSeedFolder` argument pointing to a temp folder
   pre-populated with two fixture `.vcf` files (use launch arguments to
   skip the `UIDocumentPicker` flow — bypass via a debug-only entry).
2. Verify both contacts appear in the list, alphabetically.
3. Tap "+", fill name + phone, save. Verify the new contact appears
   and a `.vcf` file exists in the temp folder.
4. Search "<partial name>" → only matching contact shown.
5. Open contact → tap Edit → change phone → save → list updates.
6. Tap Select → tap two rows → tap Delete → confirm → contacts gone,
   files gone.

Skip CNContactStore in UITests (it requires a real authorization grant
and pollutes the simulator's contacts DB). Cover sync via the unit-level
fake.

## CI

Extend `.github/workflows/build.yml` with a `test` job that runs before
`build`:

```yaml
- name: Run tests
  run: |
    xcodegen
    xcodebuild test \
      -project LocalContacts.xcodeproj \
      -scheme LocalContacts \
      -destination 'platform=iOS Simulator,name=iPhone 16,OS=latest' \
      -enableCodeCoverage YES \
      -resultBundlePath build/TestResults.xcresult
```

Fail the workflow on test failure. Upload `TestResults.xcresult` as an
artifact so failures are debuggable from PR logs.

## Phasing

Order to add tests, by ROI:

1. `VCardParser` round-trip + edge cases — protects the on-disk format.
2. `VCardWriter` + `Contact`/`PostalAddress` pure logic.
3. `ContactsStore` computed properties (`layoutMode`, `filteredContacts`,
   `allTags`).
4. `ContactsStore` file-system integration (multi-vCard sibling
   preservation, ID migration, tag rename, bulk delete).
5. `CNSyncService` pure logic (`contactDiffers`, label mapping).
6. `CNSyncService` stateful logic via `CNContactStoreProtocol` fake.
7. `processChangeEvents` / `applyMerge` extracted from views.
8. One XCUITest happy-path smoke.

Phases 1–4 are enough to catch the data-loss class of regressions on
their own; 5–7 catch the sync-correctness class; 8 is the final
canary that the app still launches and the basic flow works.
