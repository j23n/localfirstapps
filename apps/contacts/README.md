# LocalContacts

Lives at `apps/contacts` in the localfiles monorepo. Commands below are
from that directory.

A file-based contact manager for iOS. Your contacts are stored as plain vCard (.vcf) files in a folder you control — not locked into any service or cloud platform.

The headless core is `core/contacts-core` (parse/write, folder index,
Syncthing R8–R11, display rows, typed conflict state and actions).
`contacts-ffi` copies those rows onto UniFFI. The iOS shell writes through
`ContactsSession` and still parses vCard text for views and the
Apple Contacts port; that serialized read is tracked R6 debt, not a
completed boundary. Syncthing groups use `SyncConflictGroupSheet`.
Accent tokens are `design/tokens/contacts.toml` (`ContactsTokens`,
sourced light/dark). Screens are `ui-spec/screens.toml`.
The Linux shell is `shells/contacts-gtk` (`--comet` for Comet).

## Persisted values (ADR 0005 R5)

| What | Tier | Where |
|---|---|---|
| `.vcf` files | 1 | Selected folder |
| Folder event log | 2 | `.contacts/log/<dev>/YYYY-MM.ndjson` (syncs) |
| Device id | per-device | iOS: `UserDefaults` `LocalContacts_DeviceId`. Linux: `$XDG_CONFIG_HOME/localcontacts/device-id` |
| Folder bookmark / last path | per-device | iOS: `UserDefaults` security-scoped bookmark. Linux: `$XDG_CONFIG_HOME/localcontacts/folder` |

Log growth is at most one event per user save, delete, or
Syncthing-group resolve. The Tier-2 append is best-effort after the
authoritative vCard mutation. That is a gesture log; no compaction.

```
./scripts/generate_bindings.sh   # UniFFI Swift (Linux-safe)
./scripts/build_ffi.sh           # xcframework (Mac / Xcode)
```

Linux / Comet (from the monorepo root):

```
cd shells
cargo run -p contacts-gtk
cargo run -p contacts-gtk -- --comet
```

## Why

Contact data is personal and long-lived, but most contact apps store it in opaque databases tied to a specific service. LocalContacts stores each contact as a standard `.vcf` file on your filesystem. You own the files, you choose where they live, and you can read them with any text editor.

Pair it with [Syncthing](https://syncthing.net/) (via [SyncTrain](https://apps.apple.com/app/synctrain/id6475591584) on iOS) to sync your contacts across devices without any cloud service.

## Features

- **Plain vCard files** — standard `.vcf` format, readable and portable
- **Apple Contacts sync** — optionally syncs to the native Contacts app for caller ID, QuickType, and share sheets
- **Conflict detection** — Apple Contacts edits/deletions, and Syncthing `.vcf` groups (R8–R11)
- **Auto-import** — picks up contacts created in Apple Contacts
- **Full contact fields** — name, org, phone, email, address, URL, birthday, photo, notes, tags
- **Search and filter** — filter by tags, search by name/phone/email
- **No account required** — no sign-up, no server, no tracking

## Requirements

- Xcode 16.0+
- iOS 18.0+
- [XcodeGen](https://github.com/yonaskolb/XcodeGen)

## Build

```bash
# Install XcodeGen if you don't have it
brew install xcodegen

# Generate the Xcode project
xcodegen

# Open in Xcode
open LocalContacts.xcodeproj
```

Then build and run on a simulator or device (iOS 18+).

## Setup

On first launch, the app asks you to select (or create) a folder for storing `.vcf` files. This can be any folder accessible to the app — including one synced by Syncthing or iCloud Drive.

Contacts permission is optional. Without it, the app works as a standalone vCard manager. With it, contacts sync to Apple Contacts.

## AI disclaimer

Please see [docs/AI-DISCLAIMER.md]

## License

[MPL 2.0](LICENSE)
