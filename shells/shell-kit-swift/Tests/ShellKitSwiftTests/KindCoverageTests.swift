import XCTest
@testable import ShellKitSwift

final class KindCoverageTests: XCTestCase {
    func testEveryGeneratedKindHasAnExplicitDisposition() {
        for kind in ScreenKind.allCases {
            _ = ShellKitCoverage.screen(kind)
        }
        for kind in ItemKind.allCases {
            _ = ShellKitCoverage.item(kind)
        }
        for kind in Affordance.allCases {
            _ = ShellKitCoverage.affordance(kind)
        }
        for intent in NavIntent.allCases {
            _ = ShellKitCoverage.navigation(intent)
        }
        for role in ActionRole.allCases {
            _ = ShellKitCoverage.actionRole(role)
        }
        for severity in StatusSeverity.allCases {
            _ = ShellKitCoverage.statusSeverity(severity)
        }
    }

    func testOnlyPromotedScreenBindingsAreMarkedShared() {
        XCTAssertEqual(ShellKitCoverage.screen(.list), .shared)
        XCTAssertEqual(ShellKitCoverage.screen(.form), .shared)
        XCTAssertEqual(ShellKitCoverage.screen(.settings), .shared)
        XCTAssertEqual(ShellKitCoverage.screen(.grid), .appOwned)
        XCTAssertEqual(ShellKitCoverage.screen(.detail), .appOwned)
        XCTAssertEqual(ShellKitCoverage.screen(.viewer), .appOwned)
    }

    func testOnlyPromotedItemAndAffordanceBindingsAreMarkedShared() {
        XCTAssertEqual(ShellKitCoverage.item(.textRow), .shared)
        XCTAssertEqual(ShellKitCoverage.item(.fieldRow), .shared)
        XCTAssertEqual(ShellKitCoverage.item(.actionRow), .shared)
        XCTAssertEqual(ShellKitCoverage.item(.navRow), .shared)
        XCTAssertEqual(ShellKitCoverage.item(.statusRow), .shared)
        XCTAssertEqual(ShellKitCoverage.item(.mediaItem), .appOwned)

        XCTAssertEqual(ShellKitCoverage.affordance(.search), .shared)
        XCTAssertEqual(ShellKitCoverage.affordance(.filter), .shared)
        XCTAssertEqual(ShellKitCoverage.affordance(.confirm), .shared)
        XCTAssertEqual(ShellKitCoverage.affordance(.sort), .appOwned)
    }
}
