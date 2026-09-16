import Foundation
import Observation

/// People-rail domain state: hidden/featured people, the "me" person, the
/// per-person featured photo, and the derived visible-people lists. Extracted
/// from `GalleryStore` so the whole People feature lives (and can be tested)
/// in one place; views reach it via `store.people`.
///
/// The synced `.gallery/log` is authoritative after folder attach (ADR 0005
/// R5/R13). Cross-domain side effects (memory regeneration when hidden/me
/// changes, widget re-export when visibility changes) are injected as
/// closures by the Store — this type knows nothing about memories or widgets.
@Observable
@MainActor
final class PeopleStore {
    /// Person tag paths hidden from the rail, people list, and memories.
    private(set) var hiddenPeople: Set<String> = [] {
        didSet {
            onMemoryAffectingChange?()
        }
    }

    /// Person tag paths that are "featured" — sorted to the front of the
    /// People rail and decorated with a star.
    private(set) var featuredPeople: [String] = []

    /// Per-person featured photo ID. Keyed by person tag fullPath (case-sensitive).
    private(set) var featuredPhotoByPerson: [String: UUID] = [:]

    /// Person tag fullPath ("People/<name>") that represents the current user.
    /// Excluded from "with X, Y, Z" trip-title suffixes so memories don't read
    /// "Chile with Anna, Bob & me". Empty string = unset.
    private(set) var mePersonPath: String = "" {
        didSet {
            guard oldValue != mePersonPath else { return }
            // Trip titles depend on this — regenerate so the change surfaces
            // immediately instead of waiting for the daily gate.
            onMemoryAffectingChange?()
        }
    }

    /// Projected contact-link decisions.
    private(set) var personContactLinks: [String: PersonLink] = [:]

    enum Diagnostic: Equatable {
        case tornTail(path: String, offset: UInt64, detail: String)
        case migrationFailed(detail: String)
        case projectionFailed(detail: String)
        case appendFailed(operation: String, detail: String)
    }

    /// Non-fatal log health surfaced to Settings/diagnostics callers. A torn
    /// tail accompanies a recovered projection; it never triggers a stale
    /// UserDefaults fallback.
    private(set) var diagnostics: [Diagnostic] = []

    /// All People/* tags with photo counts + latest-photo dates, sorted by
    /// count. Published here by the Store after each async tag aggregation.
    private(set) var topPeople: [TagSuggestion] = []

    // MARK: Wiring

    @ObservationIgnored private let clock: any Clock
    @ObservationIgnored private let log: PersonLogBackend
    /// ADR 0005 R5: per-device, not synced.
    @ObservationIgnored let deviceId: String
    /// Library folder; log lives at `{libraryRoot}/.gallery/log/<deviceId>/`.
    @ObservationIgnored private(set) var libraryRoot: URL?
    /// The core library index: O(1) photo lookup for featured-photo IDs, and
    /// the tag → photos buckets for the candidate pool. Strong reference is
    /// cycle-free: the index doesn't know about this type.
    @ObservationIgnored private let index: CoreLibraryIndex
    /// Set by the Store: hidden/me changes affect memory generation.
    @ObservationIgnored var onMemoryAffectingChange: (() -> Void)?
    /// Set by the Store: visibility changes affect the widget snapshot.
    @ObservationIgnored var onWidgetAffectingChange: (() -> Void)?
    /// Set by the Store so projected contact links update its observed mirror.
    @ObservationIgnored var onLinksProjected: (([String: PersonLink]) -> Void)?

    init(
        defaults: UserDefaults,
        clock: any Clock,
        index: CoreLibraryIndex,
        libraryRoot: URL? = nil,
        log: PersonLogBackend = .live
    ) {
        self.clock = clock
        self.index = index
        self.deviceId = PersonLog.deviceId(in: defaults)
        self.libraryRoot = libraryRoot
        self.log = log
    }

    /// Called by the Store after each tag aggregation pass.
    func updateTopPeople(_ people: [TagSuggestion]) {
        topPeople = people
    }

    // MARK: Derived lists

    /// People/* tags for pickers (me-person, contact linking). Same content
    /// as `topPeople` — the aggregation builds the people list from the tag
    /// list, so a separate filter over the global tag list would be redundant.
    var peopleTags: [TagSuggestion] { topPeople }

    /// All people with hidden filtered out and featured floated to the front
    /// (preserves feature order). Used by PeopleListView — no cap, no recency
    /// gate so the full roster is always reachable.
    var visiblePeople: [TagSuggestion] {
        orderedVisiblePeople(recencyGated: false, cap: nil)
    }

    /// Top 20 people for the Collections rail. Non-featured must have at least
    /// one photo dated within the past 2 years; featured bypass the recency
    /// gate (the user explicitly promoted them). Sorted by total photo count.
    var visiblePeopleForRail: [TagSuggestion] {
        orderedVisiblePeople(recencyGated: true, cap: 20)
    }

    /// Shared hidden-filter + featured-first ordering behind the two lists
    /// above.
    private func orderedVisiblePeople(recencyGated: Bool, cap: Int?) -> [TagSuggestion] {
        let visible = topPeople.filter { !hiddenPeople.contains($0.fullPath) }
        let featuredSet = Set(featuredPeople)
        let featuredFirst = featuredPeople.compactMap { path in visible.first { $0.fullPath == path } }
        var rest = visible.filter { !featuredSet.contains($0.fullPath) }
        if recencyGated {
            let now = clock.now()
            let twoYearsAgo = Calendar.current.date(byAdding: .year, value: -2, to: now) ?? now
            rest = rest.filter { ($0.latestPhotoDate ?? .distantPast) > twoYearsAgo }
        }
        let ordered = featuredFirst + rest
        guard let cap else { return ordered }
        return Array(ordered.prefix(cap))
    }

    /// Hidden people resolved back to tags, for the Settings "Hidden People"
    /// list. Sorted by display name.
    var hiddenPeopleTags: [TagSuggestion] {
        hiddenPeople.compactMap { path in topPeople.first { $0.fullPath == path } }
            .sorted { $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending }
    }

    // MARK: Mutations

    struct AttachResult: Equatable {
        var state: PersonLog.State
        /// True when migration returned successfully, including the no-op
        /// "this device already has a marker" result.
        var legacyMigrationComplete: Bool
    }

    /// Bind the synced log once the library folder is known, import the
    /// legacy snapshot if needed, and always project `.gallery/log`.
    ///
    /// Projection is authoritative even when it is empty. A torn final line
    /// returns the complete prefix plus a diagnostic. A hard failure leaves
    /// this folder's state empty rather than leaking state from UserDefaults
    /// or from the previously attached library.
    @discardableResult
    func attachLibrary(_ root: URL, snapshot: PersonLog.Snapshot) -> AttachResult {
        libraryRoot = root
        diagnostics = []
        applyProjection(.init())
        var migrationComplete = false
        do {
            _ = try log.migrate(root, deviceId, snapshot)
            migrationComplete = true
        } catch {
            let detail = error.localizedDescription
            diagnostics.append(.migrationFailed(detail: detail))
            Log.cache.error("Person log migration failed: \(detail)")
        }

        do {
            let projection = try log.project(root)
            applyProjection(projection.state)
            diagnostics.append(contentsOf: projection.tornTails.map {
                .tornTail(path: $0.path, offset: $0.offset, detail: $0.detail)
            })
            for tail in projection.tornTails {
                Log.cache.error(
                    "Person log torn tail \(tail.path) at \(tail.offset): \(tail.detail)"
                )
            }
            return AttachResult(
                state: projection.state,
                legacyMigrationComplete: migrationComplete
            )
        } catch {
            let detail = error.localizedDescription
            diagnostics.append(.projectionFailed(detail: detail))
            Log.cache.error("Person log projection failed: \(detail)")
            return AttachResult(state: .init(), legacyMigrationComplete: migrationComplete)
        }
    }

    func applyProjection(_ state: PersonLog.State) {
        hiddenPeople = state.hidden
        featuredPeople = state.featured
        featuredPhotoByPerson = state.featuredPhoto.compactMapValues { UUID(uuidString: $0) }
        mePersonPath = state.me
        personContactLinks = state.personLinks()
        onLinksProjected?(personContactLinks)
    }

    @discardableResult
    private func appendPersonEvent(
        _ type: String,
        _ body: [(String, PersonLog.LogJSON)]
    ) -> Bool {
        guard let root = libraryRoot else {
            diagnostics.append(.appendFailed(operation: type, detail: "no library attached"))
            return false
        }
        do {
            try log.append(root, deviceId, type, body)
            return true
        } catch {
            let detail = error.localizedDescription
            diagnostics.append(.appendFailed(operation: type, detail: detail))
            Log.cache.error("Person log append failed (\(type)): \(detail)")
            return false
        }
    }

    @discardableResult
    func hidePerson(_ path: String) -> Bool {
        guard appendPersonEvent("person_hidden", [("path", .string(path))]) else {
            return false
        }
        hiddenPeople.insert(path)
        featuredPeople.removeAll { $0 == path }
        onWidgetAffectingChange?()
        return true
    }

    @discardableResult
    func unhidePerson(_ path: String) -> Bool {
        guard appendPersonEvent("person_unhidden", [("path", .string(path))]) else {
            return false
        }
        hiddenPeople.remove(path)
        onWidgetAffectingChange?()
        return true
    }

    func isFeatured(_ path: String) -> Bool {
        featuredPeople.contains(path)
    }

    @discardableResult
    func toggleFeaturePerson(_ path: String) -> Bool {
        if let idx = featuredPeople.firstIndex(of: path) {
            guard appendPersonEvent("person_unfeatured", [("path", .string(path))]) else {
                return false
            }
            featuredPeople.remove(at: idx)
        } else {
            guard appendPersonEvent("person_featured", [("path", .string(path))]) else {
                return false
            }
            featuredPeople.append(path)
        }
        // Featured ordering floats people to the front of the rail, which
        // the widget mirrors.
        onWidgetAffectingChange?()
        return true
    }

    func isMe(_ path: String) -> Bool {
        !mePersonPath.isEmpty && mePersonPath == path
    }

    @discardableResult
    func markAsMe(_ path: String) -> Bool {
        guard appendPersonEvent("person_me_set", [("path", .string(path))]) else {
            return false
        }
        mePersonPath = path
        return true
    }

    @discardableResult
    func unmarkAsMe() -> Bool {
        guard appendPersonEvent("person_me_clear", []) else { return false }
        mePersonPath = ""
        return true
    }

    @discardableResult
    func setFeaturedPhoto(personPath: String, photoID: UUID) -> Bool {
        guard appendPersonEvent("featured_photo_set", [
            ("path", .string(personPath)),
            ("photo", .string(photoID.uuidString)),
        ]) else {
            return false
        }
        featuredPhotoByPerson[personPath] = photoID
        onWidgetAffectingChange?()
        return true
    }

    @discardableResult
    func setContactLink(personPath: String, link: PersonLink) -> Bool {
        let body: [(String, PersonLog.LogJSON)]
        switch link {
        case .manual(let contactID):
            body = [
                ("path", .string(personPath)),
                ("contact", .string(contactID)),
            ]
        case .disabled:
            body = [
                ("path", .string(personPath)),
                ("disabled", .bool(true)),
            ]
        }
        guard appendPersonEvent("person_contact_link_set", body) else {
            return false
        }
        personContactLinks[personPath] = link
        onLinksProjected?(personContactLinks)
        onMemoryAffectingChange?()
        return true
    }

    @discardableResult
    func clearContactLink(personPath: String) -> Bool {
        guard appendPersonEvent(
            "person_contact_link_clear",
            [("path", .string(personPath))]
        ) else {
            return false
        }
        personContactLinks.removeValue(forKey: personPath)
        onLinksProjected?(personContactLinks)
        onMemoryAffectingChange?()
        return true
    }

    /// A move changes a photo's stable id (it is derived from the path).
    /// Cover photos would otherwise point at a now-missing id and fall
    /// through to the automatic pick.
    func remapFeaturedPhotoIDs(_ map: [UUID: UUID]) {
        guard !map.isEmpty else { return }
        var next = featuredPhotoByPerson
        for (path, id) in featuredPhotoByPerson {
            if let new = map[id],
               appendPersonEvent("featured_photo_set", [
                    ("path", .string(path)),
                    ("photo", .string(new.uuidString)),
               ]) {
                next[path] = new
            }
        }
        if next != featuredPhotoByPerson {
            featuredPhotoByPerson = next
        }
    }

    /// Carry every persisted decision about a person across a rename.
    ///
    /// `person_renamed` is the operation replayed by every device; install-
    /// local snapshots are neither read nor rewritten after cutover.
    @discardableResult
    func renamePerson(from old: String, to new: String) -> Bool {
        guard old != new else { return true }
        guard appendPersonEvent("person_renamed", [
            ("from", .string(old)),
            ("to", .string(new)),
        ]) else {
            return false
        }

        if hiddenPeople.contains(old) {
            hiddenPeople.remove(old)
            hiddenPeople.insert(new)
        }
        if let index = featuredPeople.firstIndex(of: old) {
            featuredPeople[index] = new
            // A rename onto somebody already featured would otherwise leave the
            // person pinned twice, which renders as two identical rail entries.
            var seen = Set<String>()
            featuredPeople = featuredPeople.filter { seen.insert($0).inserted }
        }
        if let photo = featuredPhotoByPerson.removeValue(forKey: old),
           featuredPhotoByPerson[new] == nil {
            featuredPhotoByPerson[new] = photo
        }
        if mePersonPath == old {
            mePersonPath = new
        }
        if let link = personContactLinks.removeValue(forKey: old) {
            if personContactLinks[new] == nil {
                personContactLinks[new] = link
            }
            onLinksProjected?(personContactLinks)
        }
        onWidgetAffectingChange?()
        return true
    }

    // MARK: Cover photo / face region

    /// The photo chosen as the card image for a person. When the user hasn't
    /// pinned a specific photo, prefer photos where this person is the only
    /// one tagged (cleaner cover, no other faces to crop around), then sort
    /// by recency. Face-area-based ranking turned out to be unreliable when
    /// multiple regions exist and the matching region for *this person*
    /// can't be uniquely identified by name — solo-photo preference is a
    /// simpler proxy for "good portrait of this person".
    func featuredPhoto(for tag: TagSuggestion) -> PhotoFile? {
        if let id = featuredPhotoByPerson[tag.fullPath], let photo = index.photo(byID: id) {
            return photo
        }
        let candidates = index.photos(forTag: tag)
        guard !candidates.isEmpty else { return nil }
        let solo = candidates.filter { peopleTagCount(in: $0) == 1 }
        let pool = solo.isEmpty ? candidates : solo
        return pool.max { a, b in
            (a.dateTaken ?? .distantPast) < (b.dateTaken ?? .distantPast)
        }
    }

    private func peopleTagCount(in photo: PhotoFile) -> Int {
        photo.hierarchicalTags.filter { $0.namespace?.lowercased() == "people" }.count
    }

    /// Face region matching `tag.displayName` on a candidate cover photo.
    /// Tries exact lowercased match first; falls back to first-name or
    /// substring match in case the MWG `mwg-rs:Name` value differs slightly
    /// from the `People/<name>` tag leaf (e.g. tag "Anna" but region
    /// "Anna Smith", or vice versa). Returns nil when no region's name
    /// resembles the person.
    func faceRegion(for photo: PhotoFile, person displayName: String) -> FaceRegion? {
        let target = displayName.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        guard !target.isEmpty else { return nil }
        let targetFirst = target.split(separator: " ").first.map(String.init) ?? target

        // 1. Exact lowercased match.
        if let exact = photo.faceRegions.first(where: { ($0.name?.lowercased() ?? "") == target }) {
            return exact
        }
        // 2. First-name match (handles "Anna" tag → "Anna Smith" region or vice versa).
        if let firstMatch = photo.faceRegions.first(where: { region in
            guard let name = region.name?.lowercased(), !name.isEmpty else { return false }
            let regionFirst = name.split(separator: " ").first.map(String.init) ?? name
            return regionFirst == targetFirst
        }) {
            return firstMatch
        }
        // 3. Substring match either direction.
        if let sub = photo.faceRegions.first(where: { region in
            guard let name = region.name?.lowercased(), !name.isEmpty else { return false }
            return name.contains(target) || target.contains(name)
        }) {
            return sub
        }
        return nil
    }
}
