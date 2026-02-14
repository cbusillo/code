import XCTest
import UIKit

final class CodeNativeiOSDemoUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testTopBarQuickActionsAndThreadPicker() throws {
        guard UIDevice.current.userInterfaceIdiom == .phone else {
            throw XCTSkip("This scenario is iPhone-only.")
        }

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
        let threadPickerDoneButton = app.buttons["Done"]
        XCTAssertTrue(threadPickerDoneButton.waitForExistence(timeout: 5))

        let sidebarSearch = app.textFields["sidebar.search"]
        XCTAssertTrue(sidebarSearch.exists)

        threadPickerDoneButton.tap()

        settingsButton.tap()
        let settingsDoneButton = app.buttons["settings.done"]
        XCTAssertTrue(settingsDoneButton.waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["Transcript density"].exists)
        settingsDoneButton.tap()

        let composerInput = app.textViews["composer.input"]
        XCTAssertTrue(composerInput.exists)
        composerInput.tap()
        composerInput.typeText("test clear")

        let clearButton = app.buttons["composer.clear"]
        XCTAssertTrue(clearButton.waitForExistence(timeout: 3))
        clearButton.tap()
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
        XCTAssertTrue(app.textFields["sidebar.search"].exists)

        app.buttons["rail.automations"].tap()
        let settingsDoneButton = app.buttons["settings.done"]
        XCTAssertTrue(settingsDoneButton.waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["Settings"].exists)
        XCTAssertTrue(app.staticTexts["Transcript density"].exists)
        settingsDoneButton.tap()

        app.buttons["rail.skills"].tap()
        XCTAssertTrue(settingsDoneButton.waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["Settings"].exists)
    }
}
