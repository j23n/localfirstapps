import XCTest
@testable import ShellKitSwift

final class DataSlotTests: XCTestCase {
    func testTextAndFieldSlotsPreserveDisplayData() {
        let text = ShellTextRowData(
            title: "Title",
            subtitle: "Subtitle",
            trailingValue: "Trailing",
            leadingSymbol: "doc"
        )
        XCTAssertEqual(text.title, "Title")
        XCTAssertEqual(text.subtitle, "Subtitle")
        XCTAssertEqual(text.trailingValue, "Trailing")
        XCTAssertEqual(text.leadingSymbol, "doc")

        let field = ShellFieldRowData(
            label: "Name",
            value: "Ada",
            isEditable: true
        )
        XCTAssertEqual(field.label, "Name")
        XCTAssertEqual(field.value, "Ada")
        XCTAssertTrue(field.isEditable)
    }

    func testActionNavigationStatusAndConfirmationSlotsAreOpaque() {
        let action = ShellActionRowData(
            actionID: "reload",
            label: "Reload",
            role: .destructive,
            isEnabled: false,
            leadingSymbol: "arrow.clockwise"
        )
        XCTAssertEqual(action.actionID, "reload")
        XCTAssertEqual(action.role, .destructive)
        XCTAssertFalse(action.isEnabled)

        let navigation = ShellNavRowData(
            destinationID: "logs",
            label: "Logs",
            trailingValue: "4",
            leadingSymbol: "doc"
        )
        XCTAssertEqual(navigation.destinationID, "logs")
        XCTAssertEqual(navigation.trailingValue, "4")

        let status = ShellStatusRowData(
            message: "Needs attention",
            severity: .warning,
            detail: "Try again"
        )
        XCTAssertEqual(status.severity, .warning)
        XCTAssertEqual(status.detail, "Try again")

        let confirmation = ShellConfirmData(
            actionID: "delete",
            question: "Delete item?",
            destructiveLabel: "Delete",
            message: "This cannot be undone."
        )
        XCTAssertEqual(confirmation.actionID, "delete")
        XCTAssertEqual(confirmation.destructiveLabel, "Delete")
    }

    func testActionDispatchReturnsTheOpaqueIdentifier() async {
        let received = await MainActor.run {
            var result: String?
            let dispatch = ShellActionDispatch(actionID: "reload") {
                result = $0
            }

            dispatch()
            return result
        }

        XCTAssertEqual(received, "reload")
    }

    func testFilterSelectionIsMultiSelectAndReversible() {
        let search = ShellSearchData(prompt: "Search library")
        XCTAssertEqual(search.prompt, "Search library")

        let filter = ShellFilterData(
            label: "Filter",
            options: [
                .init(id: "favorites", label: "Favorites"),
                .init(id: "recent", label: "Recent"),
            ]
        )
        XCTAssertEqual(filter.options.map(\.id), ["favorites", "recent"])

        var selection: Set<String> = ["favorites"]

        ShellFilterSelection.toggle("recent", in: &selection)
        XCTAssertEqual(selection, ["favorites", "recent"])

        ShellFilterSelection.toggle("favorites", in: &selection)
        XCTAssertEqual(selection, ["recent"])
    }

    func testTokensCarryAuthoredMetricsWithoutAColorFallback() {
        XCTAssertEqual(ShellTokens(cardRadius: 12).cardRadius, 12)
    }
}
