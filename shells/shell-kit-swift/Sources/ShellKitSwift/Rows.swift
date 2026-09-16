import SwiftUI

@MainActor
public struct ShellTextRow: View {
    public let data: ShellTextRowData

    public init(_ data: ShellTextRowData) {
        self.data = data
    }

    public var body: some View {
        HStack(spacing: 12) {
            if let symbol = data.leadingSymbol {
                Image(systemName: symbol)
                    .accessibilityHidden(true)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(data.title)
                if let subtitle = data.subtitle {
                    Text(subtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer(minLength: 8)

            if let trailingValue = data.trailingValue {
                Text(trailingValue)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

@MainActor
public struct ShellFieldRow: View {
    public let data: ShellFieldRowData
    private let editableValue: Binding<String>?

    public init(
        _ data: ShellFieldRowData,
        value: Binding<String>? = nil
    ) {
        self.data = data
        self.editableValue = value
    }

    public var body: some View {
        LabeledContent(data.label) {
            if data.isEditable, let editableValue {
                TextField(data.label, text: editableValue)
                    .labelsHidden()
                    .multilineTextAlignment(.trailing)
            } else {
                Text(data.value)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

@MainActor
public struct ShellActionRow: View {
    public let data: ShellActionRowData
    private let dispatch: ShellActionDispatch

    public init(
        _ data: ShellActionRowData,
        onAction: @escaping @MainActor (String) -> Void
    ) {
        self.data = data
        self.dispatch = ShellActionDispatch(
            actionID: data.actionID,
            callback: onAction
        )
    }

    public var body: some View {
        Button(role: buttonRole) {
            dispatch()
        } label: {
            rowLabel
        }
        .disabled(!data.isEnabled)
        .accessibilityIdentifier(data.actionID)
    }

    private var buttonRole: ButtonRole? {
        switch data.role {
        case .normal:
            nil
        case .destructive:
            .destructive
        }
    }

    @ViewBuilder
    private var rowLabel: some View {
        if let symbol = data.leadingSymbol {
            Label(data.label, systemImage: symbol)
        } else {
            Text(data.label)
        }
    }
}

@MainActor
public struct ShellNavRow<Destination: View>: View {
    public let data: ShellNavRowData
    private let destination: Destination

    public init(
        _ data: ShellNavRowData,
        @ViewBuilder destination: () -> Destination
    ) {
        self.data = data
        self.destination = destination()
    }

    public var body: some View {
        NavigationLink {
            destination
        } label: {
            ShellTextRow(
                .init(
                    title: data.label,
                    trailingValue: data.trailingValue,
                    leadingSymbol: data.leadingSymbol
                )
            )
        }
        .accessibilityIdentifier(data.destinationID)
    }
}

@MainActor
public struct ShellStatusRow: View {
    public let data: ShellStatusRowData
    @Environment(\.shellTokens) private var tokens

    public init(_ data: ShellStatusRowData) {
        self.data = data
    }

    @ViewBuilder
    public var body: some View {
        if let tokens {
            content
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
                .background(
                    .regularMaterial,
                    in: RoundedRectangle(cornerRadius: tokens.cardRadius)
                )
        } else {
            content
        }
    }

    private var content: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: severitySymbol)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(data.message)
                if let detail = data.detail {
                    Text(detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityValue(data.severity.rawValue)
    }

    private var severitySymbol: String {
        switch data.severity {
        case .info:
            "info.circle"
        case .warning:
            "exclamationmark.triangle"
        case .error:
            "xmark.octagon"
        }
    }

}
