import SwiftUI
import UIKit
import ShellKitSwift

/// In-app log viewer. Reads from `LogStore.shared`, which the `TeeLogger`
/// wrapper in `Logging.swift` populates from every `Log.<category>.<level>`
/// call. Supports level filter, text search, follow-tail toggle, copy, and
/// share-as-file. Reachable from Settings → Diagnostics.
struct LogsView: View {
    @State private var filterLevels: Set<String> = []
    @State private var searchText = ""
    @State private var isFollowing = true
    @State private var showCopyAlert = false
    @State private var logStore = LogStore.shared
    @State private var scrollPosition = ScrollPosition(edge: .bottom)

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
                return entry.message.lowercased().contains(needle)
                    || entry.category.lowercased().contains(needle)
            }
            return true
        }
    }

    var body: some View {
        contentBody
            .navigationTitle("Logs")
            .navigationBarTitleDisplayMode(.inline)
            .shellSearch(text: $searchText, data: .init(prompt: "Filter by message or category"))
            .onAppear { isFollowing = true }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
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
                ToolbarItem(placement: .topBarTrailing) { actionsMenu }
            }
            .alert("Copied", isPresented: $showCopyAlert) {
                Button("OK") {}
            } message: { Text("Logs copied to clipboard") }
    }

    private var actionsMenu: some View {
        Menu {
            Button {
                isFollowing.toggle()
            } label: {
                Label(isFollowing ? "Stop auto-scrolling" : "Auto-scroll to latest",
                      systemImage: isFollowing ? "arrow.down.circle.fill" : "arrow.down.circle")
            }
            Divider()
            Button {
                UIPasteboard.general.string = logStore.asText
                showCopyAlert = true
            } label: { Label("Copy", systemImage: "doc.on.doc") }
            Button {
                presentShareSheet()
            } label: { Label("Share", systemImage: "square.and.arrow.up") }
            Divider()
            Button(role: .destructive) {
                logStore.clear()
            } label: { Label("Clear logs", systemImage: "trash") }
        } label: {
            Image(systemName: "ellipsis.circle")
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
        ShellList {
            ForEach(filteredEntries) { entry in
                logListRow(for: entry)
            }
        }
        .listStyle(.plain)
        .scrollPosition($scrollPosition)
        .onChange(of: filteredEntries.last?.id) { _, _ in
            if isFollowing {
                scrollPosition.scrollTo(edge: .bottom)
            }
        }
        .onChange(of: isFollowing) { _, following in
            if following {
                withAnimation { scrollPosition.scrollTo(edge: .bottom) }
            }
        }
    }

    private func logListRow(for entry: LogStore.Entry) -> some View {
        let time = Self.timeFormatter.string(from: entry.timestamp)
        let trailing = entry.repeatCount > 1
            ? "\(entry.level.displayName) ×\(entry.repeatCount)"
            : entry.level.displayName
        return ShellTextRow(
            .init(
                title: entry.message,
                subtitle: "\(time)  \(entry.category)",
                trailingValue: trailing
            )
        )
    }

    private func presentShareSheet() {
        let text = logStore.asText
        let stamp = Date().formatted(.iso8601.year().month().day().dateSeparator(.dash))
        let fileName = "localcontacts-logs-\(stamp).txt"
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(fileName)
        guard (try? text.write(to: url, atomically: true, encoding: .utf8)) != nil else { return }
        ShareSheet.present(items: [url])
    }
}

#Preview {
    NavigationStack {
        ContactsRouter.destination(LogsView())
    }
}
