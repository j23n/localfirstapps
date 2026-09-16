/// `shared` means this package exposes a production SwiftUI binding.
/// `appOwned` means existing app shells still compose the native widget.
/// `nativeComposition` means SwiftUI itself owns the navigation behavior.
public enum ShellBindingDisposition: String, Equatable, Sendable {
    case shared
    case appOwned
    case nativeComposition
}

public enum ShellKitCoverage {
    public static func screen(
        _ kind: ScreenKind
    ) -> ShellBindingDisposition {
        switch kind {
        case .list:
            .shared
        case .grid:
            .appOwned
        case .detail:
            .appOwned
        case .form:
            .shared
        case .viewer:
            .appOwned
        case .settings:
            .shared
        }
    }

    public static func item(
        _ kind: ItemKind
    ) -> ShellBindingDisposition {
        switch kind {
        case .textRow:
            .shared
        case .mediaItem:
            .appOwned
        case .fieldRow:
            .shared
        case .toggleRow:
            .appOwned
        case .actionRow:
            .shared
        case .navRow:
            .shared
        case .progressRow:
            .appOwned
        case .statusRow:
            .shared
        }
    }

    public static func affordance(
        _ kind: Affordance
    ) -> ShellBindingDisposition {
        switch kind {
        case .search:
            .shared
        case .filter:
            .shared
        case .sort:
            .appOwned
        case .selection:
            .appOwned
        case .primaryAction:
            .appOwned
        case .overflow:
            .appOwned
        case .banner:
            .appOwned
        case .confirm:
            .shared
        }
    }

    public static func navigation(
        _ intent: NavIntent
    ) -> ShellBindingDisposition {
        switch intent {
        case .push:
            .nativeComposition
        case .sheet:
            .nativeComposition
        case .replace:
            .nativeComposition
        }
    }

    public static func actionRole(
        _ role: ActionRole
    ) -> ShellBindingDisposition {
        switch role {
        case .normal:
            .shared
        case .destructive:
            .shared
        }
    }

    public static func statusSeverity(
        _ severity: StatusSeverity
    ) -> ShellBindingDisposition {
        switch severity {
        case .info:
            .shared
        case .warning:
            .shared
        case .error:
            .shared
        }
    }
}
