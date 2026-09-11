import XCTest

/// Seeded-folder smoke: list → add → search → edit → delete.
/// The app skips the folder picker when launched with `--contacts-folder`.
final class LocalContactsSmokeTests: XCTestCase {

    func testListAddSearchEditDelete() throws {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("LocalContactsUI-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let alice = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:alice\r
        N:Wonder;Alice\r
        FN:Alice Wonder\r
        END:VCARD\r
        """
        try alice.write(to: folder.appendingPathComponent("alice.vcf"), atomically: true, encoding: .utf8)

        let app = XCUIApplication()
        app.launchArguments = ["--contacts-folder", folder.path]
        app.launch()

        XCTAssertTrue(app.staticTexts["Alice Wonder"].waitForExistence(timeout: 8))

        app.buttons["Add Contact"].tap()
        let firstName = app.textFields["First Name"]
        XCTAssertTrue(firstName.waitForExistence(timeout: 5))
        firstName.tap()
        firstName.typeText("Bob")
        app.buttons["Save"].tap()

        XCTAssertTrue(app.staticTexts["Bob"].waitForExistence(timeout: 5))

        let search = app.searchFields.firstMatch
        XCTAssertTrue(search.waitForExistence(timeout: 5))
        search.tap()
        search.typeText("Bob")
        XCTAssertTrue(app.staticTexts["Bob"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.staticTexts["Alice Wonder"].exists)

        app.staticTexts["Bob"].tap()
        XCTAssertTrue(app.buttons["Edit"].waitForExistence(timeout: 5))
        app.buttons["Edit"].tap()
        let lastName = app.textFields["Last Name"]
        XCTAssertTrue(lastName.waitForExistence(timeout: 5))
        lastName.tap()
        lastName.typeText("Builder")
        app.buttons["Save"].tap()

        XCTAssertTrue(app.navigationBars.buttons.firstMatch.waitForExistence(timeout: 5))
        if app.buttons["Contacts"].exists {
            app.buttons["Contacts"].tap()
        } else {
            app.navigationBars.buttons.firstMatch.tap()
        }

        if app.buttons["Cancel"].exists {
            app.buttons["Cancel"].tap()
        }

        let searchField = app.searchFields.firstMatch
        if searchField.exists {
            if searchField.buttons["Clear text"].exists {
                searchField.buttons["Clear text"].tap()
            } else {
                searchField.tap()
                searchField.typeText("")
            }
        }

        XCTAssertTrue(app.staticTexts["Alice Wonder"].waitForExistence(timeout: 5))
        app.staticTexts["Alice Wonder"].tap()
        XCTAssertTrue(app.buttons["Delete Contact"].waitForExistence(timeout: 5))
        app.buttons["Delete Contact"].tap()
        app.buttons["Delete"].tap()

        XCTAssertTrue(app.staticTexts["Bob Builder"].waitForExistence(timeout: 5)
                      || app.staticTexts["Bob"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.staticTexts["Alice Wonder"].waitForExistence(timeout: 2))
    }
}
