import SwiftUI
import UIKit
import ShellKitSwift

struct LogsView: View {
    @State private var filterLevels: Set<String> = []
    @State private var searchText = ""
    @State private var isFollowing = true
    @State private var showCopyAlert = false
    @State private var logStore = LogStore.shared

    private static let timeFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss.SSS"
        return f
    }()

    private var filteredEntries: [LogStore.Entry] {
        let needle = searchText.lowercased()
        return logStore.entries.filter { entry in
            if !filterLevels.isEmpty && !filterLevels.contains(entry.level.rawValue) {
                return false
            }
            if !needle.isEmpty {
                return entry.messageLowercased.contains(needle)
                    || entry.categoryLowercased.contains(needle)
            }
            return true
        }
    }

    var body: some View {
        contentBody
            .navigationTitle("Logs")
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier(MusicScreen.logs.rawValue)
            .shellSearch(text: $searchText, data: .init(prompt: "Filter by message or category"))
            .onAppear { isFollowing = true }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    ShellFilterMenu(
                        .init(
                            label: "Filter level",
                            options: LogStore.Entry.Level.allCases.map { level in
                                ShellFilterOption(id: level.rawValue, label: level.displayName)
                            }
                        ),
                        selection: $filterLevels
                    )
                }

                ToolbarItemGroup(placement: .topBarTrailing) {
                    Button {
                        isFollowing.toggle()
                    } label: {
                        Image(systemName: isFollowing ? "arrow.down.circle.fill" : "arrow.down.circle")
                    }
                    .help(isFollowing ? "Auto-scrolling to latest" : "Scroll to latest paused")

                    Button {
                        UIPasteboard.general.string = logStore.asText
                        showCopyAlert = true
                    } label: {
                        Image(systemName: "doc.on.doc")
                    }

                    Button {
                        presentShareSheet()
                    } label: {
                        Image(systemName: "square.and.arrow.up")
                    }
                }
            }
            .alert("Copied", isPresented: $showCopyAlert) {
                Button("OK") {}
            } message: {
                Text("Logs copied to clipboard")
            }
    }

    @ViewBuilder
    private var contentBody: some View {
        if filteredEntries.isEmpty {
            ContentUnavailableView {
                Label(searchText.isEmpty ? "No Logs" : "No Matching Logs",
                      systemImage: "doc.text.magnifyingglass")
            } description: {
                Text(searchText.isEmpty
                     ? "Logs will appear here as the app runs."
                     : "Try a different search term or filter.")
            }
        } else {
            logsList
        }
    }

    @ViewBuilder
    private var logsList: some View {
        ScrollViewReader { proxy in
            ShellList {
                ForEach(filteredEntries) { entry in
                    logListRow(for: entry)
                        .id(entry.id)
                }
            }
            .listStyle(.plain)
            .task(id: filteredEntries.last?.id) {
                guard isFollowing else { return }
                await scrollToLatest(using: proxy, animated: false)
            }
            .onChange(of: isFollowing) { _, following in
                guard following else { return }
                Task { await scrollToLatest(using: proxy, animated: true) }
            }
        }
    }

    /// Scrolls to the most recent filtered entry, deferring past the current
    /// runloop so SwiftUI's `List` has time to flush its data-source update to
    /// the underlying `UICollectionView`. Re-validates the target after the
    /// yields so a concurrent filter change or entry eviction can't ask the
    /// collection view to scroll to an index it no longer has.
    @MainActor
    private func scrollToLatest(using proxy: ScrollViewProxy, animated: Bool) async {
        guard let targetID = filteredEntries.last?.id else { return }
        await Task.yield()
        await Task.yield()
        guard !Task.isCancelled, filteredEntries.last?.id == targetID else { return }
        if animated {
            withAnimation { proxy.scrollTo(targetID, anchor: .bottom) }
        } else {
            proxy.scrollTo(targetID, anchor: .bottom)
        }
    }

    private func logListRow(for entry: LogStore.Entry) -> some View {
        let time = Self.timeFormatter.string(from: entry.timestamp)
        return ShellTextRow(
            .init(
                title: entry.message,
                subtitle: "\(time)  \(entry.category)",
                trailingValue: entry.level.displayName
            )
        )
    }

    private func presentShareSheet() {
        let text = logStore.asText
        let stamp = Date().formatted(.iso8601.year().month().day())
        let fileName = "localmusic-logs-\(stamp).txt"
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(fileName)
        guard (try? text.write(to: url, atomically: true, encoding: .utf8)) != nil else { return }
        ShareSheet.present(items: [url])
    }
}

/// Locates the topmost view controller and presents a `UIActivityViewController`,
/// handling iPad popover anchoring.
@MainActor
enum ShareSheet {
    static func present(items: [Any]) {
        let vc = UIActivityViewController(activityItems: items, applicationActivities: nil)
        guard let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene,
              let window = scene.keyWindow else { return }
        var topVC = window.rootViewController
        while let presented = topVC?.presentedViewController {
            topVC = presented
        }
        vc.popoverPresentationController?.sourceView = window
        vc.popoverPresentationController?.sourceRect = CGRect(
            x: window.bounds.midX, y: window.safeAreaInsets.top, width: 0, height: 0
        )
        topVC?.present(vc, animated: true)
    }
}

#Preview {
    NavigationStack { LogsView() }
}
