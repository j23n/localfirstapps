import SwiftUI

/// Gear button used by the top-level toolbar. Presents the Settings sheet
/// via the binding the call site already owns.
struct SettingsToolbarButton: View {
    @Binding var isPresented: Bool

    var body: some View {
        Button { isPresented = true } label: {
            Image(systemName: "gear")
        }
    }
}
