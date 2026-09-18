# LocalContactsTests

Unit + integration tests for LocalContacts. Run via the `LocalContacts`
scheme — the test target is wired into it, and the monorepo `contacts`
job in `.github/workflows/apps.yml` executes `xcodebuild test`. A
UI-test target (`LocalContactsUITests`) is also on the scheme. The nested
`.github/workflows/test.yml` is retained only for a standalone checkout; the
root `apps.yml` job is canonical in this monorepo.

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
| `ContactTests` / `PostalAddressTests` | `displayName` / `initials` / title-first `sortLetter`, `age`, `copy()` deep-copies addresses, `formatted` / `isEmpty`. |
| `ContactFieldRowsTests` | Detail grouping of core `FieldRow`s (hero skip, section titles, Phone→Notes order) and edit-draft initials. |
| `ContactsStoreTests` | `allTags`, search, tag/conflict filters, locale-aware sort, title-first `groupedContacts`, `storedDeviceId` validation, all four `layoutMode` cases, `--contacts-folder` launch-arg parsing. |
| `ContactsStoreFileSystemTests` | Load/save/delete through `ContactsSession`, ID migration, both layouts, filename collision, sibling preservation, disk sibling re-read, mixed-layout solo edit, corrupt-file sibling fallback, `save` with no folder, bulk delete, tag rewrite, Syncthing group list + auto-resolve + preview, folder log on save, `searchHits` / `fieldRows` / export. |
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
- `ContactMerge.apply` and `ContactsStore.applyChangeEvents` hold logic that used to live in views.
- `--contacts-folder <path>` skips the folder picker (`ContactsStore.folderPath(fromLaunchArguments:)`).

## Follow-up work

- **`BookmarkManager` `Sendable` honesty.** Still `@unchecked Sendable` because it stores a `UserDefaults`.
- **UI-test robustness.** The smoke test drives unlabeled system controls (search clear, back navigation). Worth accessibility identifiers if it flakes on CI.
