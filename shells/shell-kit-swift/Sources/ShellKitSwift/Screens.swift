import SwiftUI

private struct ShellTokensEnvironmentKey: EnvironmentKey {
    static let defaultValue: ShellTokens? = nil
}

public extension EnvironmentValues {
    var shellTokens: ShellTokens? {
        get { self[ShellTokensEnvironmentKey.self] }
        set { self[ShellTokensEnvironmentKey.self] = newValue }
    }
}

@MainActor
public struct ShellList<Content: View>: View {
    private let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        List {
            content
        }
    }
}

@MainActor
public struct ShellForm<Content: View>: View {
    private let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        Form {
            content
        }
    }
}

@MainActor
public struct ShellSettings<Content: View>: View {
    public let title: String
    public let tokens: ShellTokens
    public let dismissLabel: String?
    private let onDismiss: (@MainActor () -> Void)?
    private let content: Content

    public init(
        title: String,
        tokens: ShellTokens,
        dismissLabel: String? = nil,
        onDismiss: (@MainActor () -> Void)? = nil,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.tokens = tokens
        self.dismissLabel = dismissLabel
        self.onDismiss = onDismiss
        self.content = content()
    }

    public var body: some View {
        NavigationStack {
            ShellList {
                content
            }
            .navigationTitle(title)
            .modifier(ShellSettingsTitleStyle())
            .toolbar {
                if let dismissLabel, let onDismiss {
                    ToolbarItem(placement: .confirmationAction) {
                        Button(dismissLabel, action: onDismiss)
                    }
                }
            }
        }
        .environment(\.shellTokens, tokens)
    }
}

@MainActor
private struct ShellSettingsTitleStyle: ViewModifier {
    func body(content: Content) -> some View {
#if os(iOS)
        content.navigationBarTitleDisplayMode(.inline)
#else
        content
#endif
    }
}
