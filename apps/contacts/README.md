# LocalContacts

A file-based contact manager for iOS. Your contacts are stored as plain vCard (.vcf) files in a folder you control — not locked into any service or cloud platform.

## Why

Contact data is personal and long-lived, but most contact apps store it in opaque databases tied to a specific service. LocalContacts stores each contact as a standard `.vcf` file on your filesystem. You own the files, you choose where they live, and you can read them with any text editor.

Pair it with [Syncthing](https://syncthing.net/) (via [SyncTrain](https://apps.apple.com/app/synctrain/id6475591584) on iOS) to sync your contacts across devices without any cloud service.

## Features

- **Plain vCard files** — standard `.vcf` format, readable and portable
- **Apple Contacts sync** — optionally syncs to the native Contacts app for caller ID, QuickType, and share sheets
- **Conflict detection** — detects external edits/deletions and lets you choose which version to keep
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

## License

[MPL 2.0](LICENSE)
