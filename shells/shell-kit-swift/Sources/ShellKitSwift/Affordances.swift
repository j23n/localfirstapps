import SwiftUI

public extension View {
    @MainActor
    func shellSearch(
        text: Binding<String>,
        data: ShellSearchData
    ) -> some View {
        searchable(text: text, prompt: Text(data.prompt))
    }

    @MainActor
    func shellConfirmation(
        data: ShellConfirmData,
        isPresented: Binding<Bool>,
        onConfirm: @escaping @MainActor (String) -> Void
    ) -> some View {
        modifier(
            ShellConfirmationModifier(
                data: data,
                isPresented: isPresented,
                onConfirm: onConfirm
            )
        )
    }
}

@MainActor
public struct ShellFilterMenu: View {
    public let data: ShellFilterData
    @Binding private var selection: Set<String>

    public init(
        _ data: ShellFilterData,
        selection: Binding<Set<String>>
    ) {
        self.data = data
        self._selection = selection
    }

    public var body: some View {
        Menu {
            ForEach(data.options) { option in
                Button {
                    var updated = selection
                    ShellFilterSelection.toggle(option.id, in: &updated)
                    selection = updated
                } label: {
                    if selection.contains(option.id) {
                        Label(option.label, systemImage: "checkmark")
                    } else {
                        Text(option.label)
                    }
                }
            }
        } label: {
            Label(
                data.label,
                systemImage: "line.3.horizontal.decrease.circle"
            )
        }
    }
}

@MainActor
private struct ShellConfirmationModifier: ViewModifier {
    let data: ShellConfirmData
    @Binding var isPresented: Bool
    let dispatch: ShellActionDispatch

    init(
        data: ShellConfirmData,
        isPresented: Binding<Bool>,
        onConfirm: @escaping @MainActor (String) -> Void
    ) {
        self.data = data
        self._isPresented = isPresented
        self.dispatch = ShellActionDispatch(
            actionID: data.actionID,
            callback: onConfirm
        )
    }

    func body(content: Content) -> some View {
        content.confirmationDialog(
            data.question,
            isPresented: $isPresented,
            titleVisibility: .visible
        ) {
            Button(data.destructiveLabel, role: .destructive) {
                dispatch()
            }
        } message: {
            if let message = data.message {
                Text(message)
            }
        }
    }
}
