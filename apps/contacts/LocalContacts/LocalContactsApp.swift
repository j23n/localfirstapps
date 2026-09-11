import SwiftUI

@main
struct LocalContactsApp: App {
    @State private var store = ContactsStore()
    @Environment(\.scenePhase) private var scenePhase

    init() {
        CrashDiagnosticsService.shared.setEnabled(UserDefaults.standard.bool(forKey: "crashReportingEnabled"))
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(store)
                .task {
                    await store.restoreFolder()
                }
                .onChange(of: scenePhase) { _, newPhase in
                    if newPhase == .active {
                        Task {
                            await checkForExternalChanges()
                        }
                    }
                }
        }
    }

    private func checkForExternalChanges() async {
        guard store.folderURL != nil else { return }

        await store.loadContacts()

        let events = await store.syncService.fetchChanges(localContacts: store.contacts)
        await store.applyChangeEvents(events)
    }
}
