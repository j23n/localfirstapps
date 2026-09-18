import SwiftUI
import UIKit

/// Non-observed holder for the photo ids currently inside the scroll
/// viewport, fed by `onScrollTargetVisibilityChange`. A reference type so
/// per-scroll updates do NOT trigger body re-evaluation — the visible
/// date-range string is synced to @State on a debounce.
private final class VisibleTargetBox {
    var ids: [UUID] = []
}

/// Unified photo grid used for All Photos, folder/event drill-ins, tag drill-ins,
/// and memory "grid mode". Supports search, tag filtering, year scrubber,
/// Select mode with share/delete, and long-press menus — matching the Quiet design.
struct PhotoGridScreen: View {
    let title: String
    /// Static subtitle shown when no in-screen filter is active and
    /// `showVisibleDateRange` is false. When the user activates a filter
    /// (search query or tag chip), the subtitle switches to the match count;
    /// when `showVisibleDateRange` is true, the subtitle becomes a live
    /// date-range derived from the on-screen sections.
    var subtitle: String?
    /// When true, render the gear button (settings) in the toolbar.
    var isRoot: Bool = false
    /// Enable search field + tag suggestions in the header.
    var showSearch: Bool = false
    /// When true, show a live "<first> – <last>" date range derived from the
    /// sections currently scrolled into view, pinned below the nav bar so it
    /// stays visible as the user scrolls. Used by the Photos tab.
    var showVisibleDateRange: Bool = false
    /// When present, shows a "Play" button in the toolbar to jump into slideshow.
    var playableMemory: Memory? = nil
    /// When set, photo long-press offers "Set as featured image" for this person.
    var featureContextPerson: TagSuggestion? = nil
    /// Tags applied as the initial filter — used by widget deep-links that
    /// land on AllPhotos with a specific tag set already chosen.
    var initialTags: [TagSuggestion] = []
    /// Non-removable filter intent supplied by a drill-in route.
    var fixedTags: [TagSuggestion] = []
    /// Ordered id-only drill-in structure (folder or memory).
    var fixedPhotoIDs: [UUID]? = nil

    @Environment(GalleryStore.self) private var store
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @AppStorage("gridSizeTier") private var sizeTier: Int = 0

    @State private var query: String = ""
    @State private var activeTags: [TagSuggestion] = []
    @State private var scrollToTopTrigger = false
    @FocusState private var searchFocused: Bool

    // Viewer
    @State private var viewerPhoto: PhotoFile?
    @State private var viewerID = UUID()
    // Tracks the viewer's position by photo id rather than raw index. Two
    // benefits over an Int: (1) it survives transient view recreations such
    // as the foreground rescan rebuilding `filtered`, and (2) if a rescan
    // re-orders or splices photos around the current one, the viewer
    // re-resolves to the same photo at its new index instead of silently
    // landing on whatever ended up at the old index. Initial UUID() is a
    // placeholder — never read because `openViewer(at:)` writes the real id
    // before the cover binds to `viewerPhoto`.
    @State private var viewerCurrentPhotoID: UUID = UUID()

    // Select mode
    @State private var selectMode = false
    @State private var selected: Set<UUID> = []
    @State private var hasSeededInitialTags = false

    // Large title collapsed → show inline principal with date range
    @State private var largeTitleCollapsed = false

    // Pinch-to-zoom grid size
    @State private var isPinching = false
    @State private var pinchBaseTier: Int = 0

    // Share request (drives the unified size-selection share flow)
    @State private var shareRequest: PhotoShareRequest?
    /// Photos the delete alert is about. Captured when the user taps Delete
    /// so a selection change cannot rewrite the confirmation mid-flight.
    @State private var pendingDelete: [PhotoFile] = []
    @State private var showDeleteConfirm = false
    @State private var pendingMove: [PhotoFile] = []
    @State private var showMovePicker = false

    // Settings sheet (root only)
    @State private var showSettings = false

    // Slideshow navigation
    @State private var goToSlideshow = false

    // Materialized only when the viewer is explicitly opened.
    @State private var filtered: [PhotoFile] = []
    @State private var windowStructure: ViewStructure?
    @State private var windowItems: [String: GalleryMediaItem] = [:]

    // Photo ids currently visible in the viewport. Stored in a reference
    // type so per-scroll mutations do NOT trigger body re-evaluation —
    // critical for the 20k All Photos grid. The date-range @State string is
    // synced on a debounce (~300ms).
    @State private var visibleTargets = VisibleTargetBox()
    /// `photo.id` → display date for content windows that have arrived.
    @State private var dateByID: [UUID: Date] = [:]
    @State private var liveDateRange: String = ""
    @State private var dateRangeUpdateTask: Task<Void, Never>?

    /// Anchors for the grid-cell → viewer zoom transition (iOS 18
    /// `matchedTransitionSource` / `navigationTransition(.zoom)`).
    @Namespace private var zoomNamespace

    private func grid(isLandscape: Bool) -> GridLayoutConfig {
        GridLayoutConfig(sizeTier: sizeTier, isLandscape: isLandscape)
    }

    // View intent identity. `epoch` catches each rebuilt library; the legacy
    // count/date fields stay in the value type to keep its conformance fixture
    // source-compatible while windowed routes use ids and generations.
    struct FilterKey: Equatable {
        let count: Int
        let firstDate: Date?
        let lastDate: Date?
        let query: String
        let activeTagIDs: [String]
        let epoch: Int
    }

    private var filterKey: FilterKey {
        FilterKey(
            count: fixedPhotoIDs?.count ?? 0,
            firstDate: nil,
            lastDate: nil,
            query: query.trimmingCharacters(in: .whitespaces),
            activeTagIDs: (fixedTags + activeTags).map(\.id),
            epoch: store.libraryEpoch
        )
    }

    /// True when the user has narrowed the grid via the search field or a tag
    /// chip — i.e. `filtered.count` no longer reflects the full input set.
    private var isFiltered: Bool {
        !activeTags.isEmpty || !query.trimmingCharacters(in: .whitespaces).isEmpty
    }

    private var windowItemIDs: [String] {
        windowStructure?.sections.flatMap(\.itemIds) ?? []
    }

    private var displayedCount: Int {
        windowItemIDs.count
    }

    private var displayedPhotoIDs: [UUID] {
        windowItemIDs.compactMap(UUID.init(uuidString:))
    }

    private var windowYears: [(year: String, sectionID: String)] {
        var seen = Set<String>()
        return (windowStructure?.sections ?? []).compactMap { section in
            guard let year = section.title.split(separator: " ").last.map(String.init),
                  year.count == 4,
                  seen.insert(year).inserted else { return nil }
            return (year, section.id)
        }
    }

    /// The title shown in the navigation bar — collapses to the select-mode
    /// status when in select mode so the same chrome conveys both states.
    private var displayTitle: String {
        if selectMode {
            return selected.isEmpty ? "Select Items" : "\(selected.count) Selected"
        }
        return title
    }

    /// The subtitle line rendered just under the large nav title. Three modes,
    /// in priority order: select-mode hint > visible-date-range (Photos tab) >
    /// match-count (when filtered) > caller-supplied static subtitle.
    private var displaySubtitle: String? {
        if selectMode {
            return selected.isEmpty
                ? "Tap photos to select"
                : "Tap Share to send \(selected.count == 1 ? "this photo" : "these photos")"
        }
        if showVisibleDateRange {
            return liveDateRange
        }
        if isFiltered {
            let n = displayedCount
            return "\(n) \(n == 1 ? "match" : "matches")"
        }
        return subtitle
    }

    /// Whether the subtitle bar should currently render visible content. Both
    /// the in-content variant and the pinned safeAreaInset variant gate on
    /// this so they stay in sync.
    private var hasSubtitleContent: Bool {
        guard let sub = displaySubtitle else { return false }
        return !sub.isEmpty
    }

    /// Debounced sync of the non-observed visible-target box to the @State
    /// `liveDateRange` string. At most one body re-evaluation per 300ms
    /// instead of one per visibility change — keeps scroll smooth on the
    /// 20k All Photos grid.
    private func scheduleDateRangeUpdate() {
        dateRangeUpdateTask?.cancel()
        dateRangeUpdateTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            let text = computeDateRange()
            if text != liveDateRange { liveDateRange = text }
        }
    }

    /// Compute the date range from the currently visible photo ids.
    private func computeDateRange() -> String {
        var minDate: Date?
        var maxDate: Date?
        for id in visibleTargets.ids {
            guard let d = dateByID[id] else { continue }
            if minDate == nil || d < minDate! { minDate = d }
            if maxDate == nil || d > maxDate! { maxDate = d }
        }
        guard let first = minDate, let last = maxDate else { return "" }
        if Calendar.current.isDate(first, inSameDayAs: last) {
            return Self.dateRangeFormatter.string(from: first)
        }
        return "\(Self.dateRangeFormatter.string(from: first)) – \(Self.dateRangeFormatter.string(from: last))"
    }

    /// Shared `DateFormatter` for the visible-date-range subtitle. Stable per
    /// process — avoids re-allocating on every body re-evaluation.
    private static let dateRangeFormatter: DateFormatter = {
        let fmt = DateFormatter()
        fmt.setLocalizedDateFormatFromTemplate("d MMM yyyy")
        return fmt
    }()

    private static let windowDateFormatter: ISO8601DateFormatter = {
        let fmt = ISO8601DateFormatter()
        fmt.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return fmt
    }()

    private var suggestions: [TagSuggestion] {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty else { return [] }
        let needle = q.lowercased()
        let activeIDs = Set((fixedTags + activeTags).map(\.id))
        let tags = store.allTags.filter {
            !activeIDs.contains($0.id) &&
            ($0.displayName.lowercased().contains(needle) || $0.fullPath.lowercased().contains(needle))
        }
        return Array((tags + dateSuggestions(matching: needle))
            .sorted { $0.count > $1.count }
            .prefix(6))
    }

    /// Year / month buckets that match `needle`. Ids are `date:YYYY` /
    /// `date:YYYY-MM` so a tap can become a date-shaped search query.
    private func dateSuggestions(matching needle: String) -> [TagSuggestion] {
        var years: [Int: Int] = [:]
        var months: [String: (title: String, count: Int)] = [:]
        let calendar = Calendar(identifier: .gregorian)
        for photo in store.sortedPhotos {
            guard let date = photo.dateTaken else { continue }
            let year = calendar.component(.year, from: date)
            let month = calendar.component(.month, from: date)
            years[year, default: 0] += 1
            let key = String(format: "%04d-%02d", year, month)
            let title = "\(Self.monthNames[max(0, min(month, 12) - 1)]) \(year)"
            var entry = months[key, default: (title, 0)]
            entry.count += 1
            months[key] = entry
        }
        var hits: [TagSuggestion] = []
        for (year, count) in years where String(year).contains(needle) {
            hits.append(TagSuggestion(
                id: String(format: "date:%04d", year),
                displayName: String(year),
                fullPath: "Date/\(year)",
                namespace: "Date",
                count: count
            ))
        }
        for (key, value) in months
            where value.title.lowercased().contains(needle) || key.contains(needle) {
            hits.append(TagSuggestion(
                id: "date:\(key)",
                displayName: value.title,
                fullPath: "Date/\(value.title)",
                namespace: "Date",
                count: value.count
            ))
        }
        return hits
    }

    private static let monthNames = [
        "January", "February", "March", "April", "May", "June",
        "July", "August", "September", "October", "November", "December"
    ]

    var body: some View {
        GeometryReader { geo in
            let width = geo.size.width
            let isLandscape = verticalSizeClass == .compact
            let grid = grid(isLandscape: isLandscape)
            let cell = grid.cellSize(for: width)

            ScrollViewReader { proxy in
                ScrollView {
                    Color.clear.frame(height: 0).id("__top__")

                    // On the Photos tab the live date-range subtitle lives in
                    // the toolbar .principal — keep it OUT of scroll content so
                    // per-cell visibleSectionCounts mutations don't force
                    // LazyVGrid layout invalidation every frame.
                    if !showVisibleDateRange || selectMode, hasSubtitleContent {
                        inContentSubtitle(displaySubtitle ?? "")
                    }

                    if !activeTags.isEmpty {
                        tagPills
                    }
                    if showSearch {
                        searchField
                        if !suggestions.isEmpty {
                            suggestionsList
                        }
                    }

                    if windowStructure == nil {
                        ProgressView()
                            .padding(.top, 48)
                    } else if displayedCount == 0 {
                        ContentUnavailableView(
                            "No photos match.",
                            systemImage: "photo.stack"
                        )
                        .padding(.top, 48)
                    } else if let structure = windowStructure {
                        LazyVGrid(columns: grid.columns(for: width), spacing: 2) {
                            ForEach(structure.sections, id: \.id) { section in
                                Section {
                                    ForEach(section.itemIds.indices, id: \.self) { offset in
                                        windowedGridCell(
                                            id: section.itemIds[offset],
                                            sectionID: section.id,
                                            offset: offset,
                                            generation: structure.generation,
                                            cellSize: cell
                                        )
                                    }
                                } header: {
                                    if structure.sections.count > 1 && !section.title.isEmpty {
                                        Text(section.title.uppercased())
                                            .font(.system(size: 12.5, weight: .semibold))
                                            .tracking(0.2)
                                            .foregroundStyle(Design.ink2)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .padding(.horizontal, 20)
                                            .padding(.top, 14)
                                            .padding(.bottom, 6)
                                            .id(section.id)
                                    }
                                }
                            }
                        }
                        .scrollTargetLayout()
                    }
                }
                .scrollDismissesKeyboard(.interactively)
                .onScrollGeometryChange(for: Bool.self) { geo in
                    geo.contentOffset.y > 20
                } action: { _, collapsed in
                    largeTitleCollapsed = collapsed
                }
                // Replaces the per-cell onAppear/onDisappear bookkeeping that
                // previously fed the live date-range subtitle: one callback
                // with the full visible id set instead of 2 closures per cell,
                // and it re-fires when the *content* changes under a static
                // viewport (filter edits), which onAppear never did — that was
                // the stale-subtitle bug. Threshold 0.1 ≈ the old onAppear
                // semantics (partially visible rows count).
                .onScrollTargetVisibilityChange(idType: UUID.self, threshold: 0.1) { visibleIDs in
                    guard showVisibleDateRange else { return }
                    visibleTargets.ids = visibleIDs
                    scheduleDateRangeUpdate()
                }
                .refreshable { await store.rescan(kind: .light) }
                .overlay(alignment: .trailing) {
                    if windowYears.count > 1 && !selectMode {
                        YearScrubber(years: windowYears) { sectionID in
                            withAnimation { proxy.scrollTo(sectionID, anchor: .top) }
                        }
                    }
                }
                .onChange(of: scrollToTopTrigger) {
                    withAnimation { proxy.scrollTo("__top__", anchor: .top) }
                }
                .simultaneousGesture(
                    MagnifyGesture()
                        .onChanged { value in
                            let scale = value.magnification
                            if !isPinching {
                                isPinching = true
                                pinchBaseTier = sizeTier
                            }
                            let delta: Int
                            if scale > 1.5 { delta = -2 }
                            else if scale > 1.15 { delta = -1 }
                            else if scale < 0.67 { delta = 2 }
                            else if scale < 0.85 { delta = 1 }
                            else { delta = 0 }
                            sizeTier = max(0, min(GridLayoutConfig.tierCount - 1, pinchBaseTier + delta))
                        }
                        .onEnded { _ in isPinching = false }
                )
            }
        }
        .background(Design.bg)
        .navigationTitle(displayTitle)
        .navigationBarTitleDisplayMode(isRoot ? .large : .inline)
        .toolbar { toolbarContent }
        // Photos-style select: the tab bar (Folders / Collections / Photos)
        // yields the slot entirely, and the share / count / select-all bar
        // sits in its place via safeAreaInset — not a translucent overlay
        // stacked on top of the tabs.
        .toolbar(selectMode ? .hidden : .automatic, for: .tabBar)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if selectMode { selectBottomBar }
        }
        .task(id: filterKey) {
            // Seed only once per deep-link mount. Using a flag rather than
            // `activeTags.isEmpty` prevents re-seeding when the user removes
            // all tag chips (which also makes activeTags empty).
            if !initialTags.isEmpty && !hasSeededInitialTags {
                hasSeededInitialTags = true
                activeTags = initialTags
                return
            }
            refreshWindowedView()
            if showVisibleDateRange { scheduleDateRangeUpdate() }
        }
        .onDisappear {
            dateRangeUpdateTask?.cancel()
            dateRangeUpdateTask = nil
        }
        .fullScreenCover(item: $viewerPhoto) { _ in
            PhotoViewerView(photos: filtered, currentPhotoID: $viewerCurrentPhotoID)
                .id(viewerID)
                // Photos-style zoom from the tapped cell. Keyed on the *live*
                // current id so paging in the viewer re-targets the dismissal
                // zoom at the new photo's cell; if that cell isn't on screen,
                // the system falls back to a plain fade.
                .navigationTransition(.zoom(sourceID: viewerCurrentPhotoID, in: zoomNamespace))
        }
        .sheet(isPresented: $showSettings) { SettingsView() }
        .photoShareSheet(request: $shareRequest)
        .alert(PhotoDeletePrompt.title(for: pendingDelete), isPresented: $showDeleteConfirm) {
            Button("Cancel", role: .cancel) { pendingDelete = [] }
            Button("Delete", role: .destructive) {
                Task {
                    await confirmDelete(pendingDelete)
                    pendingDelete = []
                }
            }
        } message: {
            Text(PhotoDeletePrompt.message(for: pendingDelete))
        }
        .sheet(isPresented: $showMovePicker) {
            FolderMovePicker(photos: pendingMove) { dest in
                Task {
                    await confirmMove(pendingMove, to: dest)
                    pendingMove = []
                }
            }
        }
        .navigationDestination(isPresented: $goToSlideshow) {
            if let m = playableMemory {
                MemorySlideshowView(memory: m)
            }
        }
    }

    // MARK: - Subtitle (in-content for static, pinned for live date range)

    private func inContentSubtitle(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 13))
            .foregroundStyle(Design.ink3)
            .lineLimit(1)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 20)
            .padding(.top, 4)
            .padding(.bottom, 8)
    }

    private var tagPills: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(activeTags) { tag in
                    HStack(spacing: 5) {
                        Image(systemName: tag.icon)
                            .font(.system(size: 9.5))
                        Text(tag.displayName)
                            .font(.system(size: 11.5, weight: .medium))
                        Button {
                            activeTags.removeAll { $0.id == tag.id }
                        } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 9, weight: .bold))
                        }
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .foregroundStyle(Design.accentColor)
                    .background(Design.accentSoft, in: Capsule())
                }
            }
            .padding(.horizontal, 20)
        }
        .padding(.bottom, 8)
    }

    private var searchField: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(Design.ink3)
            TextField("Search by name or tag", text: $query)
                .font(.system(size: 15))
                .tint(Design.accentColor)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .focused($searchFocused)
                .submitLabel(.done)
                .onSubmit { searchFocused = false }
            if !query.isEmpty {
                Button {
                    query = ""
                    searchFocused = false
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(Design.ink3)
                }
            } else if searchFocused {
                Button("Done") {
                    searchFocused = false
                }
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Design.accentColor)
            }
        }
        .padding(.horizontal, 12)
        .frame(height: 36)
        .background(Color(red: 0.235, green: 0.216, blue: 0.176).opacity(0.06), in: RoundedRectangle(cornerRadius: 10))
        .padding(.horizontal, 16)
        .padding(.bottom, 8)
    }

    private var suggestionsList: some View {
        VStack(spacing: 0) {
            ForEach(Array(suggestions.enumerated()), id: \.element.id) { idx, tag in
                Button {
                    if tag.namespace?.lowercased() == "date" {
                        query = String(tag.id.dropFirst("date:".count))
                    } else {
                        activeTags.append(tag)
                        query = ""
                    }
                } label: {
                    HStack(spacing: 10) {
                        Image(systemName: tag.icon)
                            .font(.system(size: 13))
                            .foregroundStyle(Design.accentColor)
                        Text(tag.displayName)
                            .font(.system(size: 14.5))
                            .foregroundStyle(Design.ink)
                        Spacer()
                        Text("\(tag.count)")
                            .font(.system(size: 12))
                            .foregroundStyle(Design.ink3)
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                }
                .buttonStyle(.plain)
                if idx < suggestions.count - 1 {
                    Divider().background(Design.separator).padding(.leading, 14)
                }
            }
        }
        .background(Design.bgCard)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .shadow(color: Color.black.opacity(0.03), radius: 1, y: 1)
        .padding(.horizontal, 16)
        .padding(.bottom, 10)
    }

    // MARK: - Grid cell

    @ViewBuilder
    private func windowedGridCell(
        id: String,
        sectionID: String,
        offset: Int,
        generation: UInt64,
        cellSize: CGFloat
    ) -> some View {
        let photoID = UUID(uuidString: id)!
        let item = windowItems[id]
        let isSelected = selected.contains(photoID)

        ZStack {
            if let item {
                ThumbnailView(
                    url: URL(fileURLWithPath: item.thumbnailRef),
                    size: cellSize,
                    isVideo: item.badge == "Video",
                    isLivePhoto: item.badge == "Live Photo"
                )
                .accessibilityLabel(item.accessibilityLabel ?? "Photo")
            } else {
                Rectangle()
                    .fill(Design.bgCard)
                    .overlay { ProgressView().controlSize(.small) }
            }

            if selectMode {
                Rectangle()
                    .fill(isSelected ? Design.accentColor.opacity(0.18) : .clear)
                VStack {
                    Spacer()
                    HStack {
                        Spacer()
                        Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                            .font(.system(size: 22))
                            .foregroundStyle(isSelected ? Design.accentColor : .white)
                            .padding(6)
                    }
                }
            }
        }
        .frame(width: cellSize, height: cellSize)
        .contentShape(Rectangle())
        .id(photoID)
        .matchedTransitionSource(id: photoID, in: zoomNamespace)
        .task(id: "\(generation):\(offset / 64)") {
            loadPhotoWindow(
                sectionID: sectionID,
                offset: offset,
                generation: generation
            )
        }
        .onTapGesture {
            if selectMode {
                if selected.contains(photoID) { selected.remove(photoID) }
                else { selected.insert(photoID) }
            } else if let photo = store.index.photo(byID: photoID) {
                openViewer(at: photo)
            }
        }
        .contextMenu {
            if !selectMode,
               let photo = store.index.photo(byID: photoID) {
                Button { openViewer(at: photo) } label: {
                    Label("Open", systemImage: "eye")
                }
                PhotoShareMenu(
                    canResize: !photo.isVideo,
                    onSelect: { quality in
                        shareRequest = PhotoShareRequest(photos: [photo], quality: quality)
                    }
                ) {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
                Button {
                    withAnimation(.easeInOut(duration: 0.25)) {
                        selectMode = true
                        selected.insert(photoID)
                    }
                } label: {
                    Label("Select", systemImage: "checkmark.circle")
                }
                if let person = featureContextPerson {
                    Button {
                        store.people.setFeaturedPhoto(
                            personPath: person.fullPath,
                            photoID: photoID
                        )
                    } label: {
                        Label("Set as featured image", systemImage: "star")
                    }
                }
            }
        }
    }

    /// Fetch one 64-item chunk. Every cell in the chunk shares the same task
    /// id and the dictionary guard below, so a visible row crosses FFI once.
    /// A refused/stale read refreshes structure instead of mixing content from
    /// two generations.
    @MainActor
    private func loadPhotoWindow(sectionID: String, offset: Int, generation: UInt64) {
        let chunkOffset = offset / 64 * 64
        guard windowStructure?.generation == generation else { return }
        let section = windowStructure?.sections.first { $0.id == sectionID }
        guard let section, section.itemIds.indices.contains(offset) else { return }
        let requestedID = section.itemIds[offset]
        guard windowItems[requestedID] == nil else { return }
        do {
            let rows = try store.index.photoWindow(
                sectionID: sectionID,
                offset: chunkOffset,
                limit: 64,
                generation: generation
            )
            for row in rows {
                windowItems[row.id] = row
                if let id = UUID(uuidString: row.id),
                   let label = row.label,
                   let date = Self.windowDateFormatter.date(from: label) {
                    dateByID[id] = date
                }
            }
            if showVisibleDateRange { scheduleDateRangeUpdate() }
        } catch {
            refreshWindowedView()
        }
    }

    private func openViewer(at photo: PhotoFile) {
        // Domain payload is materialized only for the explicit viewer action.
        if fixedPhotoIDs != nil {
            filtered = store.index.photos(withIDs: displayedPhotoIDs)
        } else {
            filtered = store.index.search(
                query: query,
                requiredTags: fixedTags + activeTags
            )
        }
        viewerCurrentPhotoID = photo.id
        viewerID = UUID()
        viewerPhoto = photo
    }

    private func confirmDelete(_ photos: [PhotoFile]) async {
        let result = await store.deletePhotos(photos)
        selected.subtract(result.deletedIDs)
        if selected.isEmpty {
            withAnimation(.easeInOut(duration: 0.25)) {
                selectMode = false
            }
        }
    }

    private func confirmMove(_ photos: [PhotoFile], to dest: PhotoFolder) async {
        let result = await store.movePhotos(photos, to: dest)
        selected.subtract(Set(result.moved.keys))
        if selected.isEmpty {
            withAnimation(.easeInOut(duration: 0.25)) {
                selectMode = false
            }
        }
    }

    // MARK: - Toolbar

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItem(placement: .topBarLeading) {
            if selectMode {
                Button("Cancel") {
                    withAnimation(.easeInOut(duration: 0.25)) {
                        selectMode = false
                        selected.removeAll()
                    }
                }
                .foregroundStyle(Design.accentColor)
            }
        }

        // Principal slot: scan / tagging progress takes priority over the
        // date-range title button while either is running, so the user sees
        // live "X / Y · ~M:SS" instead of a stale date range. The branch
        // lives inside `PrincipalToolbarContent` so those reads stay scoped
        // to that view — otherwise every progress tick would re-evaluate
        // this entire screen body and re-diff every visible grid cell,
        // starving thumbnail `.task` closures on the main actor.
        ToolbarItem(placement: .principal) {
            PrincipalToolbarContent(
                title: title,
                displaySubtitle: displaySubtitle,
                showVisibleDateRange: showVisibleDateRange,
                selectMode: selectMode,
                isRoot: isRoot,
                largeTitleCollapsed: largeTitleCollapsed,
                onTitleTap: { scrollToTopTrigger.toggle() }
            )
        }

        ToolbarItemGroup(placement: .topBarTrailing) {
            if selectMode {
                Button {
                    if selected.count == displayedCount {
                        selected.removeAll()
                    } else {
                        selected = Set(displayedPhotoIDs)
                    }
                } label: {
                    Text(selected.count == displayedCount && displayedCount > 0 ? "Deselect All" : "Select All")
                }
                .fontWeight(.semibold)
                .foregroundStyle(Design.accentColor)
                .disabled(displayedCount == 0)
            } else {
                if playableMemory != nil {
                    Button {
                        goToSlideshow = true
                    } label: {
                        Label("Play", systemImage: "play.fill")
                    }
                    .labelStyle(.titleAndIcon)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Design.accentColor)
                }

                if isRoot {
                    SettingsToolbarButton(isPresented: $showSettings)
                }
            }
        }
    }

    // MARK: - Select bottom bar

    private var selectBottomBar: some View {
        let selectedPhotos = displayedPhotoIDs
            .filter(selected.contains)
            .compactMap { store.index.photo(byID: $0) }
        let canResize = !selectedPhotos.isEmpty && selectedPhotos.allSatisfy { !$0.isVideo }

        return HStack {
            PhotoShareMenu(
                canResize: canResize,
                onSelect: { quality in
                    guard !selectedPhotos.isEmpty else { return }
                    shareRequest = PhotoShareRequest(photos: selectedPhotos, quality: quality)
                }
            ) {
                HStack(spacing: 6) {
                    Image(systemName: "square.and.arrow.up")
                    Text("Share")
                }
                .font(.system(size: 16, weight: .medium))
                .foregroundStyle(selected.isEmpty ? Design.ink3 : Design.accentColor)
            }
            .disabled(selected.isEmpty)

            Spacer()

            Text(selected.isEmpty
                 ? "Select items"
                 : "\(photoCountLabel(selected.count)) selected")
                .font(.system(size: 13))
                .foregroundStyle(Design.ink2)

            Spacer()

            Button {
                pendingMove = selectedPhotos
                showMovePicker = true
            } label: {
                Text("Move")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(selected.isEmpty ? Design.ink3 : Design.accentColor)
            }
            .disabled(selected.isEmpty)

            Button {
                pendingDelete = selectedPhotos
                showDeleteConfirm = true
            } label: {
                Text("Delete")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(selected.isEmpty ? Design.ink3 : Design.destructive)
            }
            .disabled(selected.isEmpty)
        }
        .padding(.horizontal, 16)
        .frame(maxWidth: .infinity, minHeight: 49)
        .background {
            // Same recipe as the tab bar (`configureAppearance`): nearly
            // opaque canvas, separator on top, extending into the home-
            // indicator inset so it reads as the bar that replaced the tabs.
            Rectangle()
                .fill(Design.bg.opacity(0.96))
                .ignoresSafeArea(edges: .bottom)
                .overlay(alignment: .top) {
                    Rectangle()
                        .fill(Design.separator)
                        .frame(height: 0.5)
                }
        }
    }

    // MARK: - Filter & sort (off-main)

    @MainActor
    private func refreshWindowedView() {
        let structure: ViewStructure
        if let fixedPhotoIDs {
            structure = store.index.photoIDsView(
                id: title,
                photoIDs: fixedPhotoIDs,
                query: query,
                requiredTags: fixedTags + activeTags
            )
        } else {
            structure = store.index.photoView(
                query: query,
                requiredTags: fixedTags + activeTags
            )
        }
        guard structure.generation != windowStructure?.generation else { return }
        windowItems.removeAll(keepingCapacity: true)
        dateByID.removeAll(keepingCapacity: true)
        windowStructure = structure
    }

}

// MARK: - Principal toolbar content

/// Wraps the principal toolbar slot so the scan / analysis progress reads
/// stay scoped to this view's body. If `PhotoGridScreen.body` read them
/// directly (which it used to, via the `if store.scanProgress != nil`
/// branch), every progress tick would re-evaluate the whole screen body —
/// re-diffing every visible cell on the main actor and starving the
/// thumbnail `.task` closures that are queued on it.
///
/// Library scan *and* photo analysis (tagging / faces / places) both win
/// over the date-range title. The Photos tab used to gate on `scanProgress`
/// alone, so a tagging run left this slot showing the date range while
/// Folders and Collections already showed the banner.
private struct PrincipalToolbarContent: View {
    @Environment(GalleryStore.self) private var store
    let title: String
    let displaySubtitle: String?
    let showVisibleDateRange: Bool
    let selectMode: Bool
    let isRoot: Bool
    let largeTitleCollapsed: Bool
    let onTitleTap: () -> Void

    private var isProgressVisible: Bool {
        store.progressRevealed
            && (store.scanProgress != nil || store.analysis.progress != nil)
    }

    var body: some View {
        if isProgressVisible {
            ScanProgressBanner()
        } else if showVisibleDateRange && !selectMode {
            Button(action: onTitleTap) {
                VStack(spacing: 1) {
                    Text(title)
                        .font(.headline)
                        .fontWeight(.semibold)
                    if let sub = displaySubtitle, !sub.isEmpty {
                        Text(sub)
                            .font(.caption2)
                            .foregroundStyle(Design.ink2)
                    }
                }
                .foregroundStyle(Design.ink)
            }
            .buttonStyle(.plain)
            // Root uses .large title mode — fade the principal in only
            // after the large title has scrolled away to avoid duplication.
            .opacity(isRoot && !largeTitleCollapsed ? 0 : 1)
            .animation(.easeInOut(duration: 0.15), value: largeTitleCollapsed)
        }
    }
}

// MARK: - Year scrubber

struct YearScrubber: View {
    let years: [(year: String, sectionID: String)]
    let onScrollTo: (String) -> Void

    @State private var isDragging = false
    @State private var lastIndex = -1

    var body: some View {
        GeometryReader { geo in
            VStack(spacing: 0) {
                ForEach(Array(years.enumerated()), id: \.element.year) { idx, entry in
                    Text("'" + entry.year.suffix(2))
                        .font(.system(size: 10, weight: .bold, design: .rounded))
                        .foregroundStyle(isDragging && idx == lastIndex
                                         ? Design.accentColor
                                         : (isDragging ? Design.ink : Design.ink3))
                        .scaleEffect(isDragging && idx == lastIndex ? 1.4 : 1.0)
                        .frame(maxHeight: .infinity)
                        .animation(.spring(response: 0.25, dampingFraction: 0.7), value: lastIndex)
                }
            }
            .frame(maxHeight: .infinity)
            .padding(.horizontal, 4)
            .background(
                Capsule()
                    .fill(.ultraThinMaterial)
                    .opacity(isDragging ? 1 : 0)
            )
            .animation(.easeOut(duration: 0.2), value: isDragging)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        isDragging = true
                        let index = Int(value.location.y / geo.size.height * CGFloat(years.count))
                        let clamped = max(0, min(years.count - 1, index))
                        if clamped != lastIndex {
                            lastIndex = clamped
                            onScrollTo(years[clamped].sectionID)
                            UISelectionFeedbackGenerator().selectionChanged()
                        }
                    }
                    .onEnded { _ in
                        isDragging = false
                        lastIndex = -1
                    }
            )
        }
        .frame(width: 28)
        .padding(.vertical, 40)
    }
}

