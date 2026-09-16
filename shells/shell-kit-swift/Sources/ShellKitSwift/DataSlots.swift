import SwiftUI

/// App-authored metrics passed into native bindings. The app's asset-catalog
/// accent remains the SwiftUI tint, so the kit never manufactures a colour.
public struct ShellTokens: Equatable, Sendable {
    public let cardRadius: CGFloat

    public init(cardRadius: CGFloat) {
        self.cardRadius = cardRadius
    }
}

public struct ShellTextRowData: Equatable, Sendable {
    public let title: String
    public let subtitle: String?
    public let trailingValue: String?
    public let leadingSymbol: String?

    public init(
        title: String,
        subtitle: String? = nil,
        trailingValue: String? = nil,
        leadingSymbol: String? = nil
    ) {
        self.title = title
        self.subtitle = subtitle
        self.trailingValue = trailingValue
        self.leadingSymbol = leadingSymbol
    }
}

public struct ShellFieldRowData: Equatable, Sendable {
    public let label: String
    public let value: String
    public let isEditable: Bool

    public init(label: String, value: String, isEditable: Bool) {
        self.label = label
        self.value = value
        self.isEditable = isEditable
    }
}

public struct ShellActionRowData: Equatable, Sendable {
    public let actionID: String
    public let label: String
    public let role: ActionRole
    public let isEnabled: Bool
    public let leadingSymbol: String?

    public init(
        actionID: String,
        label: String,
        role: ActionRole = .normal,
        isEnabled: Bool = true,
        leadingSymbol: String? = nil
    ) {
        self.actionID = actionID
        self.label = label
        self.role = role
        self.isEnabled = isEnabled
        self.leadingSymbol = leadingSymbol
    }
}

public struct ShellNavRowData: Equatable, Sendable {
    public let destinationID: String
    public let label: String
    public let trailingValue: String?
    public let leadingSymbol: String?

    public init(
        destinationID: String,
        label: String,
        trailingValue: String? = nil,
        leadingSymbol: String? = nil
    ) {
        self.destinationID = destinationID
        self.label = label
        self.trailingValue = trailingValue
        self.leadingSymbol = leadingSymbol
    }
}

public struct ShellStatusRowData: Equatable, Sendable {
    public let message: String
    public let severity: StatusSeverity
    public let detail: String?

    public init(
        message: String,
        severity: StatusSeverity,
        detail: String? = nil
    ) {
        self.message = message
        self.severity = severity
        self.detail = detail
    }
}

public struct ShellSearchData: Equatable, Sendable {
    public let prompt: String

    public init(prompt: String) {
        self.prompt = prompt
    }
}

public struct ShellFilterOption: Equatable, Identifiable, Sendable {
    public let id: String
    public let label: String

    public init(id: String, label: String) {
        self.id = id
        self.label = label
    }
}

public struct ShellFilterData: Equatable, Sendable {
    public let label: String
    public let options: [ShellFilterOption]

    public init(label: String, options: [ShellFilterOption]) {
        self.label = label
        self.options = options
    }
}

public struct ShellChartRowData: Equatable, Sendable {
    public let title: String
    public let subtitle: String?
    public let unit: String?
    public let latest: String?
    public let values: [Double]

    public init(
        title: String,
        subtitle: String? = nil,
        unit: String? = nil,
        latest: String? = nil,
        values: [Double]
    ) {
        self.title = title
        self.subtitle = subtitle
        self.unit = unit
        self.latest = latest
        self.values = values
    }
}

public struct ShellConfirmData: Equatable, Sendable {
    public let actionID: String
    public let question: String
    public let destructiveLabel: String
    public let message: String?

    public init(
        actionID: String,
        question: String,
        destructiveLabel: String,
        message: String? = nil
    ) {
        self.actionID = actionID
        self.question = question
        self.destructiveLabel = destructiveLabel
        self.message = message
    }
}

/// Keeps action identity opaque to the kit while making dispatch behavior
/// independently testable from SwiftUI view rendering.
@MainActor
public struct ShellActionDispatch {
    public let actionID: String
    private let callback: @MainActor (String) -> Void

    public init(
        actionID: String,
        callback: @escaping @MainActor (String) -> Void
    ) {
        self.actionID = actionID
        self.callback = callback
    }

    public func callAsFunction() {
        callback(actionID)
    }
}

public enum ShellFilterSelection {
    public static func toggle(_ id: String, in selection: inout Set<String>) {
        if selection.contains(id) {
            selection.remove(id)
        } else {
            selection.insert(id)
        }
    }
}
