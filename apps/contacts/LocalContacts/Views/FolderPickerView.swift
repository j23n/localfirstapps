import SwiftUI
import UIKit

struct FolderPickerView: View {
    @Environment(ContactsStore.self) private var store
    @State private var showPicker = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                Spacer()

                Image(systemName: "folder.badge.person.crop")
                    .font(.system(size: 64))
                    .foregroundStyle(.secondary)

                Text("Welcome to LocalContacts")
                    .font(.title2.bold())

                Text("Select a folder containing your .vcf contact files, or an empty folder to start fresh.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 32)

                Button {
                    showPicker = true
                } label: {
                    Label("Choose Folder", systemImage: "folder")
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .padding(.horizontal, 48)

                Spacer()
            }
            .navigationTitle("LocalContacts")
            .sheet(isPresented: $showPicker) {
                DocumentPickerView { url in
                    Task {
                        await store.setFolder(url)
                    }
                }
            }
        }
    }
}

struct DocumentPickerView: UIViewControllerRepresentable {
    let onPick: (URL) -> Void

    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.folder])
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ uiViewController: UIDocumentPickerViewController, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(onPick: onPick)
    }

    class Coordinator: NSObject, UIDocumentPickerDelegate {
        let onPick: (URL) -> Void

        init(onPick: @escaping (URL) -> Void) {
            self.onPick = onPick
        }

        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
            guard let url = urls.first else { return }
            onPick(url)
        }
    }
}
