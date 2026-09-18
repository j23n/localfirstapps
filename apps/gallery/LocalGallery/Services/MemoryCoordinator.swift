import Foundation
import Observation
import os

/// Owns the memories domain: the generated `[Memory]` list, its disk cache,
/// the once-per-day generation gate, the seen/cool-down bookkeeping, and the
/// hidden-memories set. Extracted from `GalleryStore` so the trickiest state
/// machine in the app lives (and can be tested) in one place; views reach it
/// via `store.memories`.
///
/// The engine itself lives in the Rust core (`CoreMemories.generate`, Phase 4)
/// and is pure; this type supplies its inputs (via the Store-provided
/// `makeInputs` closure), owns the gating rules, and publishes results.
/// After folder attach, hidden / seen / surfaced / birthdays / generated-day
/// are projected from `.gallery/log` and are not written to UserDefaults.
@Observable
@MainActor
final class MemoryCoordinator {
    /// Everything the engine needs from the *Store*, snapshotted on the main
    /// actor at generation time. Built by the Store, which owns the photo
    /// library, contacts, and people state.
    ///
    /// The coordinator's own half of the snapshot — the clock, the seed, the
    /// birthdays toggle and the seen/cool-down maps — is added in `generate`;
    /// together they become `CoreMemories.Inputs`, which is what crosses the
    /// FFI boundary.
    struct GenerationInputs {
        let photos: [PhotoFile]
        let leafFolders: [PhotoFolder]
        let contacts: [ContactInfo]
        let personContactLinks: [String: PersonLink]
        let mePersonPath: String
        let hiddenPeople: Set<String>
    }

    /// The full generated list, cached order. Most UI wants `visible`.
    private(set) var all: [Memory] = []

    /// Memory IDs the user has hidden ("Don't show this memory").
    private(set) var hiddenMemories: Set<String> = [] {
        didSet { persistHiddenMemories() }
    }

    /// Memory IDs the user has tapped into (opened the slideshow). Keyed by
    /// memory ID → last-seen date. Memories seen within ~6 months are
    /// deprioritised so the rail stays fresh. Pruned at load to entries
    /// inside the last 12 months — entries older than that have no scoring
    /// effect (the engine only checks `> sixMonthsAgo`) and would otherwise
    /// accumulate forever as the user uses the app across years.
    @ObservationIgnored var seenMemoryIDs: [String: Date] = [:] {
        didSet { persistSeenMemoryIDs() }
    }

    /// Cluster keys (see `CoreMemories.clusterKey(for:)`) → last date the
    /// cluster surfaced on the rail. Clusters are penalised for ~3 days
    /// after surfacing so a trip parent + sub-trips rotate across days.
    /// Pruned at load to entries from the last 7 days; that headroom
    /// covers the 3-day cool-down with slack for time-zone changes.
    @ObservationIgnored var surfacedClusters: [String: Date] = [:] {
        didSet { persistSurfacedClusters() }
    }

    /// Master toggle for birthday memories. When `false`, generation skips
    /// the birthday category entirely. Default `true` so the feature
    /// surfaces automatically once Contacts access is granted.
    var birthdaysEnabled: Bool = true {
        didSet {
            guard oldValue != birthdaysEnabled else { return }
            guard persistBirthdaysEnabled(previous: oldValue) else { return }
            // Force a regenerate so today's rail reflects the toggle change
            // immediately rather than waiting for the daily gate.
            if !applyingProjection {
                forceRegenerate()
            }
        }
    }

    /// Day the last successful generation ran. Persisted; the once-per-day
    /// gate compares it against today. Only set *after* a generation
    /// completes — a process death or cancelled BG task must not consume
    /// the day.
    @ObservationIgnored private(set) var generatedDay: Date? {
        didSet { persistGeneratedDay() }
    }

    /// Re-entrancy guard: `generateIfNeeded` spawns an unawaited Task and
    /// the daily gate is only set on completion, so without this two
    /// triggers in quick succession would both generate.
    @ObservationIgnored private var isGenerating = false
    /// Bumped by `forceRegenerate` so an in-flight generation (whose inputs
    /// predate the change that forced the regen) can't set the daily gate
    /// when it lands.
    @ObservationIgnored private var generationEpoch = 0
    /// The in-flight generation, retained so `forceRegenerate` can cancel it
    /// rather than only bumping the epoch and hoping the old Task notices.
    @ObservationIgnored private var generationTask: Task<Void, Never>?
    /// Seed for the generation that should replace a cancelled in-flight
    /// run. Drained when the cancelled task releases `isGenerating`.
    @ObservationIgnored private var pendingGenerationSeed: String?

    #if DEBUG
    /// Test seam: runs after the engine returns and before the epoch /
    /// cancellation publish checks, so a force-regen can interleave.
    @ObservationIgnored var testStallBeforePublish: (@MainActor () async -> Void)?
    #endif

    // MARK: Wiring

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let clock: any Clock
    @ObservationIgnored private let cache: JSONDiskCache<[Memory]>
    /// O(1) photo lookup for stale-ID filtering in `visible`.
    @ObservationIgnored private let index: CoreLibraryIndex
    /// Hidden-people set, for suppressing stale birthday memories.
    @ObservationIgnored private let people: PeopleStore
    @ObservationIgnored private let log: MemoryLogBackend
    /// ADR 0005 R5: per-device, shared with `PersonLog`.
    @ObservationIgnored let deviceId: String
    /// Library folder; log lives at `{libraryRoot}/.gallery/log/<deviceId>/`.
    @ObservationIgnored private(set) var libraryRoot: URL?
    /// After folder attach, the five keys are log-backed and must not be
    /// written to UserDefaults.
    @ObservationIgnored private(set) var logBacked = false
    @ObservationIgnored private var applyingProjection = false

    enum Diagnostic: Equatable {
        case tornTail(path: String, offset: UInt64, detail: String)
        case migrationFailed(detail: String)
        case projectionFailed(detail: String)
        case appendFailed(operation: String, detail: String)
    }

    /// Non-fatal log health. A torn tail accompanies a recovered projection;
    /// it never triggers a stale UserDefaults fallback.
    private(set) var diagnostics: [Diagnostic] = []
    /// Set by the Store: snapshots engine inputs, or nil when the library
    /// is empty (nothing to generate from).
    @ObservationIgnored var makeInputs: (() -> GenerationInputs?)?
    /// Set by the Store: fired after a generation publishes, so the widget
    /// snapshot can refresh.
    @ObservationIgnored var onMemoriesPublished: (() -> Void)?

    init(
        defaults: UserDefaults,
        clock: any Clock,
        cache: JSONDiskCache<[Memory]>,
        index: CoreLibraryIndex,
        people: PeopleStore,
        log: MemoryLogBackend = .live
    ) {
        self.defaults = defaults
        self.clock = clock
        self.cache = cache
        self.index = index
        self.people = people
        self.log = log
        self.deviceId = MemoryLog.deviceId(in: defaults)

        if let hiddenMem = defaults.array(forKey: "hiddenMemories") as? [String] {
            hiddenMemories = Set(hiddenMem)
        }
        if let raw = defaults.object(forKey: "memoriesGeneratedDay") as? Date {
            generatedDay = raw
        }
        if let data = defaults.data(forKey: "seenMemoryIDs"),
           let dict = try? JSONDecoder().decode([String: Date].self, from: data) {
            // Drop entries older than the engine's scoring window so the dict
            // can't grow unbounded across years of use.
            seenMemoryIDs = pruneSeen(dict)
        }
        if let data = defaults.data(forKey: "surfacedClusters"),
           let dict = try? JSONDecoder().decode([String: Date].self, from: data) {
            // Drop entries outside the cool-down window so the map can't
            // grow unbounded across years of use.
            surfacedClusters = pruneSurfaced(dict)
        }
        if defaults.object(forKey: "birthdayMemoriesEnabled") != nil {
            birthdaysEnabled = defaults.bool(forKey: "birthdayMemoriesEnabled")
        }
        if let cached = cache.load() {
            all = cached
            Log.cache.info("Loaded \(cached.count) memories from cache")
        }
    }

    struct AttachResult: Equatable {
        var state: MemoryLog.State
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
    func attachLibrary(_ root: URL, snapshot: MemoryLog.Snapshot) -> AttachResult {
        libraryRoot = root
        logBacked = true
        diagnostics = []
        applyProjection(.init())
        var migrationComplete = false
        do {
            _ = try log.migrate(root, deviceId, snapshot)
            migrationComplete = true
        } catch {
            let detail = error.localizedDescription
            diagnostics.append(.migrationFailed(detail: detail))
            Log.cache.error("Memory log migration failed: \(detail)")
        }

        do {
            let projection = try log.project(root)
            applyProjection(projection.state)
            diagnostics.append(contentsOf: projection.tornTails.map {
                .tornTail(path: $0.path, offset: $0.offset, detail: $0.detail)
            })
            for tail in projection.tornTails {
                Log.cache.error(
                    "Memory log torn tail \(tail.path) at \(tail.offset): \(tail.detail)"
                )
            }
            return AttachResult(
                state: projection.state,
                legacyMigrationComplete: migrationComplete
            )
        } catch {
            let detail = error.localizedDescription
            diagnostics.append(.projectionFailed(detail: detail))
            Log.cache.error("Memory log projection failed: \(detail)")
            return AttachResult(state: .init(), legacyMigrationComplete: migrationComplete)
        }
    }

    func applyProjection(_ state: MemoryLog.State) {
        applyingProjection = true
        hiddenMemories = state.hidden
        seenMemoryIDs = pruneSeen(state.seen)
        surfacedClusters = pruneSurfaced(state.surfaced)
        birthdaysEnabled = state.birthdaysEnabled
        generatedDay = state.generatedDay
        applyingProjection = false
    }

    // MARK: Reads

    /// Rail-facing list: hidden memories removed, stale birthday memories
    /// for since-hidden people removed, and memories whose photo IDs are
    /// entirely unresolvable (folder moves change SHA-256 IDs) removed.
    var visible: [Memory] {
        all.filter { memory in
            guard !hiddenMemories.contains(memory.id) else { return false }
            // Suppress birthday memories for people who have since been hidden
            // (catches stale cached memories generated before the person was hidden).
            if memory.id.hasPrefix("birthday-") {
                let personPath = String(memory.id.dropFirst("birthday-".count))
                if people.hiddenPeople.contains(personPath) { return false }
            }
            // Mirror the same resolution order as MemoryCardView's coverURL.
            if index.photo(byID: memory.coverPhotoID) != nil { return true }
            return memory.photoIDs.contains { index.photo(byID: $0) != nil }
        }
    }

    /// True once today's generation has completed. `AppRouter` uses this to
    /// decide whether a widget deep link targeting a not-yet-existing
    /// scheduled memory should stay queued (today's generation still
    /// pending) or be dropped (the memory is genuinely gone).
    var hasGeneratedToday: Bool {
        guard let day = generatedDay else { return false }
        return Calendar.current.isDate(day, inSameDayAs: clock.now())
    }

    // MARK: Mutations

    func hide(_ id: String) {
        guard commitMemoryEvent("memory_hidden", [("id", .string(id))]) else { return }
        hiddenMemories.insert(id)
        onMemoriesPublished?()
    }

    func unhide(_ id: String) {
        guard commitMemoryEvent("memory_unhidden", [("id", .string(id))]) else { return }
        hiddenMemories.remove(id)
        onMemoriesPublished?()
    }

    func markSeen(_ id: String) {
        let now = clock.now()
        guard commitMemoryEvent("memory_seen", [
            ("id", .string(id)),
            ("at", .string(MemoryLog.formatDate(now))),
        ]) else { return }
        seenMemoryIDs[id] = now
    }

    /// Wipe the disk cache without touching in-memory state. The Store calls
    /// this when the *library* cache is evicted — memories reference photo
    /// IDs from that schema, so the persisted copy goes too, while the
    /// in-memory list keeps rendering (filtered by `visible`) until the
    /// rescan regenerates.
    func clearDiskCache() {
        cache.clear()
    }

    // MARK: Generation

    /// Trigger generation once per day (or when the list is empty). Called
    /// after scan/enrichment so memories reflect the freshest EXIF/tag/GPS
    /// data. Fire-and-forget; the daily gate is set on completion.
    func generateIfNeeded(seed: String? = nil) {
        let cal = Calendar.current
        let today = cal.startOfDay(for: clock.now())
        let stale = generatedDay.map { !cal.isDate($0, inSameDayAs: today) } ?? true
        guard stale || all.isEmpty else { return }
        guard let inputs = makeInputs?(), !inputs.photos.isEmpty else { return }
        let daySeed = seed ?? WidgetDayKey.string(for: today)
        guard !isGenerating else { return }
        startGeneration(inputs: inputs, seed: daySeed, today: today)
    }

    /// Clear the once-per-day gate + cached memories and immediately re-run
    /// detection. Wired to settings toggles and a long-press on the
    /// "Memories" section header. Uses a time-based seed so each tap gives
    /// a fresh selection.
    ///
    /// No-op when no photos are loaded yet (`makeInputs` returns nil/empty)
    /// — there's nothing to regenerate before the library cache loads, and
    /// the first scan's own `generateIfNeeded` populates from scratch.
    func forceRegenerate() {
        guard makeInputs?()?.photos.isEmpty == false else { return }
        generationEpoch += 1
        generatedDay = nil
        all = []
        cache.clear()
        Log.memory.info("Force-regenerating memories")
        let seed = "\(clock.now().timeIntervalSinceReferenceDate)"
        generationTask?.cancel()
        pendingGenerationSeed = seed
        if !isGenerating {
            drainPendingGeneration()
        }
    }

    /// Background-task entry point: same once-per-day gate as the foreground
    /// path, but awaited so the BG handler knows when to call
    /// `setTaskCompleted`. The Store refreshes contacts before calling this.
    func runScheduledRefresh() async {
        guard let inputs = makeInputs?(), !inputs.photos.isEmpty else {
            Log.bg.info("No photos in memory; skipping background memory generation")
            return
        }
        let today = Calendar.current.startOfDay(for: clock.now())
        let alreadyToday = generatedDay.map { Calendar.current.isDate($0, inSameDayAs: today) } ?? false
        if alreadyToday {
            Log.bg.info("Memories already generated today; skipping BG regeneration")
            return
        }
        if isGenerating {
            await generationTask?.value
            await waitForGenerationToIdle()
            return
        }
        let task = startGeneration(
            inputs: inputs,
            seed: WidgetDayKey.string(for: today),
            today: today
        )
        await task.value
        // A force-regen that cancelled this run queues a replacement from
        // the task's cleanup; the BG handler must wait for that too.
        await waitForGenerationToIdle()
    }

    private func waitForGenerationToIdle() async {
        while isGenerating {
            await generationTask?.value
        }
    }

    @discardableResult
    private func startGeneration(
        inputs: GenerationInputs,
        seed: String,
        today: Date?
    ) -> Task<Void, Never> {
        isGenerating = true
        let epoch = generationEpoch
        let task = Task { [weak self] in
            guard let self else { return }
            await self.generate(inputs: inputs, seed: seed, epoch: epoch)
            // Gate only on completion — a process death or cancellation
            // mid-generation must not consume the daily gate; an epoch bump
            // means a force-regen superseded this run's inputs.
            if !Task.isCancelled && epoch == self.generationEpoch, let today {
                self.generatedDay = today
            }
            self.isGenerating = false
            self.generationTask = nil
            self.drainPendingGeneration()
        }
        generationTask = task
        return task
    }

    private func drainPendingGeneration() {
        guard let seed = pendingGenerationSeed else { return }
        pendingGenerationSeed = nil
        generateIfNeeded(seed: seed)
    }

    private func generate(inputs: GenerationInputs, seed: String, epoch: Int) async {
        let t = CFAbsoluteTimeGetCurrent()

        CoreMemories.logInputSummary(allPhotos: inputs.photos)

        // Only the bounded platform context crosses inside `generate`; the
        // photo table stays retained by `LibraryIndex`. `contactsByLowerName`
        // is not passed: the core derives
        // it from `contacts` exactly as `ContactLinker.index` does (lowercased
        // full name, first write wins), and shipping both would let the two
        // disagree across the boundary.
        let results = await CoreMemories.generate(CoreMemories.Inputs(
            photos: inputs.photos,
            leafFolders: inputs.leafFolders,
            contacts: inputs.contacts,
            personContactLinks: inputs.personContactLinks,
            birthdaysEnabled: birthdaysEnabled,
            mePersonPath: inputs.mePersonPath,
            hiddenPeople: inputs.hiddenPeople,
            now: clock.now(),
            seed: seed,
            seenMemoryIDs: seenMemoryIDs,
            surfacedClusters: surfacedClusters
        ), using: index.retainedLibrary())

        #if DEBUG
        if let stall = testStallBeforePublish {
            await stall()
        }
        #endif

        // An expired BG task cancels mid-generation — don't publish a
        // potentially partial result set over a good cached one. An epoch
        // bump means a force-regen invalidated these inputs; writing the
        // cache here would resurrect memories `forceRegenerate` just cleared.
        guard !Task.isCancelled else {
            Log.memory.info("Memory generation cancelled; discarding results")
            return
        }
        guard epoch == generationEpoch else {
            Log.memory.info("Memory generation superseded; discarding results")
            return
        }

        let elapsed = (CFAbsoluteTimeGetCurrent() - t) * 1000
        all = results
        // Record the clusters we just surfaced so the next generation
        // applies the cool-down penalty to their members.
        let now = clock.now()
        var updated = surfacedClusters
        for memory in results {
            let key = CoreMemories.clusterKey(for: memory.id)
            guard commitMemoryEvent("memory_cluster_surfaced", [
                ("key", .string(key)),
                ("at", .string(MemoryLog.formatDate(now))),
            ]) else { continue }
            updated[key] = now
        }
        surfacedClusters = updated
        cache.save(all)
        Log.memory.info("Generated \(results.count) memories in \(String(format: "%.0f", elapsed))ms")

        // Memories changed → refresh the widget snapshot so the Memories
        // widget picks up new "On this day" / "Years ago" content the same
        // day they become valid.
        onMemoriesPublished?()
    }

    // MARK: Persistence

    private func persistHiddenMemories() {
        guard !applyingProjection, !logBacked else { return }
        defaults.set(Array(hiddenMemories), forKey: "hiddenMemories")
    }

    private func persistSeenMemoryIDs() {
        guard !applyingProjection, !logBacked else { return }
        guard let data = try? JSONEncoder().encode(seenMemoryIDs) else { return }
        defaults.set(data, forKey: "seenMemoryIDs")
    }

    private func persistSurfacedClusters() {
        guard !applyingProjection, !logBacked else { return }
        guard let data = try? JSONEncoder().encode(surfacedClusters) else { return }
        defaults.set(data, forKey: "surfacedClusters")
    }

    @discardableResult
    private func persistBirthdaysEnabled(previous: Bool) -> Bool {
        guard !applyingProjection else { return true }
        if logBacked {
            if !appendMemoryEvent("memory_birthdays_set", [
                ("enabled", .bool(birthdaysEnabled)),
            ]) {
                applyingProjection = true
                birthdaysEnabled = previous
                applyingProjection = false
                return false
            }
            return true
        }
        defaults.set(birthdaysEnabled, forKey: "birthdayMemoriesEnabled")
        return true
    }

    private func persistGeneratedDay() {
        guard !applyingProjection else { return }
        if logBacked {
            if let day = generatedDay {
                _ = appendMemoryEvent("memory_generated_day", [
                    ("day", .string(MemoryLog.formatDate(day))),
                ])
            } else {
                _ = appendMemoryEvent("memory_generated_day_clear", [])
            }
            return
        }
        if let day = generatedDay {
            defaults.set(day, forKey: "memoriesGeneratedDay")
        } else {
            defaults.removeObject(forKey: "memoriesGeneratedDay")
        }
    }

    /// Pre-attach mutations stay install-local. After attach, append first.
    private func commitMemoryEvent(
        _ type: String,
        _ body: [(String, MemoryLog.LogJSON)]
    ) -> Bool {
        guard logBacked, !applyingProjection else { return true }
        return appendMemoryEvent(type, body)
    }

    @discardableResult
    private func appendMemoryEvent(
        _ type: String,
        _ body: [(String, MemoryLog.LogJSON)]
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
            Log.cache.error("Memory log append failed (\(type)): \(detail)")
            return false
        }
    }

    private func pruneSeen(_ dict: [String: Date]) -> [String: Date] {
        let cutoff = Calendar.current.date(byAdding: .month, value: -12, to: clock.now()) ?? .distantPast
        return dict.filter { $0.value > cutoff }
    }

    private func pruneSurfaced(_ dict: [String: Date]) -> [String: Date] {
        let cutoff = Calendar.current.date(byAdding: .day, value: -7, to: clock.now()) ?? .distantPast
        return dict.filter { $0.value > cutoff }
    }
}
