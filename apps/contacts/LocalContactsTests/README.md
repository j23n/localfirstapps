# LocalContactsTests

Unit + integration tests for LocalContacts. Run via the `LocalContacts`
scheme — the test target is wired into it, and CI executes
`xcodebuild test`. A UI-test target (`LocalContactsUITests`) is also on
the scheme.

## Conventions

- **Temp dirs per test.** File-system tests build a unique folder under
  `FileManager.default.temporaryDirectory` and remove it via `defer`.
  Never touch the user-selected folder.
- **No `UserDefaults.standard`.** `BookmarkManager` and `CNSyncService`
  accept an injected `UserDefaults`; tests pass a per-test
  `UserDefaults(suiteName:)`.
- **Skip `setFolder`.** It goes through bookmarks, security-scoped
  resources, and `UserDefaults`. File-system tests assign
  `store.folderURL` directly and call `loadContacts` / `save` / etc.
- **Fake the contact store.** Authorization-gated `CNContactStore` work
  goes through `CNContactStoreProtocol`. Tests inject `FakeCNContactStore`.
- **Swift Testing** (`@Test`, `#expect`, `#require`) for unit tests.
  The smoke UI test uses XCTest.

## Current coverage

| Suite | What it covers |
| --- | --- |
| `VCardParserTests` | Round-trip identity, header skipping, name components, type-label extraction, group prefix, addresses, BDAY formats, base64 PHOTO + URI fallback + folded PHOTO, CATEGORIES, X-LOCALCONTACTS-ID, unknown-field preservation, single-pass unescape (`\\n` vs `\n`), line folding, `parseMultiple` (order, blanks, empty `Data`, BEGIN without END, junk between cards, stray BEGIN), both `assignDefaultID` branches. |
| `VCardWriterTests` | Header order + CRLF, optional-field omission, escaping (NOTE, FN, TEL/EMAIL/URL), TYPE sanitization, BDAY, PHOTO folding, unknown-field round-trip, filename suggestion, end-to-end write→parse. |
| `ContactTests` / `PostalAddressTests` | `displayName` / `initials` / `sortLetter`, `age`, `copy()` deep-copies addresses, `formatted` / `isEmpty`. |
| `ContactsStoreTests` | `allTags`, search, tag/conflict filters, locale-aware sort, `groupedContacts`, all four `layoutMode` cases, `--contacts-folder` launch-arg parsing. |
| `ContactsStoreFileSystemTests` | Load/save/delete against temp folders, ID migration, both layouts, filename collision, sibling preservation, disk sibling re-read, mixed-layout solo edit, corrupt-file sibling fallback, `save` with no folder, bulk delete, tag rewrite. |
| `ContactsStoreChangeEventTests` | `applyChangeEvents`: update/delete conflict state (no overwrite), empty list, unknown IDs, added → import + CN claim. |
| `ContactMergeTests` | Field-by-field Apple/local selection, list replace (not merge-by-index), birthday, photo-only-if-nil, `conflictState` left intact. |
| `BookmarkManagerTests` | save/load, `hasBookmark` / `clearBookmark`, corrupt stored data. |
| `CNSyncServiceTests` | Pure `cnLabel` / `vCardLabel` / `contactDiffers` logic. |
| `CNSyncServiceStoreTests` | `pushContact` first vs update vs denied, `deleteContact` mapped/unmapped, `fetchChanges` token short-circuit / deleted / updated labels / added / fetch error keeps token, `fullReconciliation`, `claimCNContact`, `.limited` is not full access. |
| `ContactDetailURLTests` | `dialURL`, `mailURL` (including space encoding), `websiteURL` (bare host + `HTTPS://`). |
| `LocalContactsUITests` | Seeded-folder smoke: list → add → search → edit → delete. |

## Production-side accommodations

- `BookmarkManager` accepts an injected `UserDefaults`; `bookmarkKey` is `internal static`.
- `CNSyncService` takes `CNContactStoreProtocol` + `UserDefaults`. Production uses `CNContactStoreAdapter`.
- Identifier / group fetches are explicit protocol methods (not opaque `NSPredicate`) so the fake can implement them.
- `VCardParser.parse` / `parseMultiple` take `assignDefaultID: Bool = true`.
- `ContactMerge.apply` and `ContactsStore.applyChangeEvents` hold logic that used to live in views.
- `--contacts-folder <path>` skips the folder picker (`ContactsStore.folderPath(fromLaunchArguments:)`).

## Follow-up work

- **`BookmarkManager` `Sendable` honesty.** Still `@unchecked Sendable` because it stores a `UserDefaults`.
- **UI-test robustness.** The smoke test drives unlabeled system controls (search clear, back navigation). Worth accessibility identifiers if it flakes on CI.
