import SwiftUI

struct ContentView: View {
    @Environment(ContactsStore.self) private var store

    var body: some View {
        Group {
            if store.folderURL != nil {
                ContactListView()
            } else {
                FolderPickerView()
            }
        }
    }
}
