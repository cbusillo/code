import XCTest
import UIKit

final class CodeNativeiOSDemoUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testTopBarQuickActionsAndThreadPicker() throws {
        let app = XCUIApplication()
        app.launch()

        let quickActionsButton = app.buttons["top.quick-actions"]
        let threadsButton = app.buttons["top.threads"]
        let settingsButton = app.buttons["top.settings"]

        XCTAssertTrue(quickActionsButton.waitForExistence(timeout: 10))
        XCTAssertTrue(threadsButton.exists)
        XCTAssertTrue(settingsButton.exists)

        quickActionsButton.tap()
        XCTAssertTrue(app.buttons["top.quick-actions.new-thread"].waitForExistence(timeout: 5))
        let refreshAction = app.buttons["top.quick-actions.refresh"]
        XCTAssertTrue(refreshAction.exists)
        refreshAction.tap()

        threadsButton.tap()
        XCTAssertTrue(app.buttons["Done"].waitForExistence(timeout: 5))
    }

    @MainActor
    func testIPadSplitLayoutShowsPersistentSidebar() throws {
        guard UIDevice.current.userInterfaceIdiom == .pad else {
            throw XCTSkip("This scenario is iPad-only.")
        }

        let app = XCUIApplication()
        app.launch()

        XCTAssertTrue(app.buttons["rail.new-thread"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.textViews["composer.input"].exists)
        XCTAssertFalse(app.buttons["top.threads"].exists)
    }
}
