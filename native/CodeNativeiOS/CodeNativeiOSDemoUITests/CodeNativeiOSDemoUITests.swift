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

        XCTAssertTrue(quickActionsButton.waitForExistence(timeout: 10))

        quickActionsButton.tap()
        XCTAssertTrue(app.buttons["top.quick-actions.new-thread"].waitForExistence(timeout: 5))
        let refreshAction = app.buttons["top.quick-actions.refresh"]
        XCTAssertTrue(refreshAction.exists)
        refreshAction.tap()

        if app.buttons["top.threads"].exists {
            app.buttons["top.threads"].tap()
        } else {
            quickActionsButton.tap()
            let threadsMenuItem = app.buttons["top.threads"]
            XCTAssertTrue(threadsMenuItem.waitForExistence(timeout: 5))
            threadsMenuItem.tap()
        }

        let threadPickerDoneButton = app.buttons["Done"]
        XCTAssertTrue(threadPickerDoneButton.waitForExistence(timeout: 5))

        let sidebarSearch = app.textFields["sidebar.search"]
        let hasThreadPickerEmptyState = app.staticTexts["No threads"].exists || app.staticTexts["No matching threads"].exists
        XCTAssertTrue(sidebarSearch.exists || hasThreadPickerEmptyState)

        threadPickerDoneButton.tap()

        if app.buttons["top.settings"].exists {
            app.buttons["top.settings"].tap()
        } else {
            quickActionsButton.tap()
            let settingsMenuItem = app.buttons["top.settings"]
            XCTAssertTrue(settingsMenuItem.waitForExistence(timeout: 5))
            settingsMenuItem.tap()
        }

        let settingsDoneButton = app.buttons["settings.done"]
        XCTAssertTrue(settingsDoneButton.waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["Transcript density"].exists)
        settingsDoneButton.tap()

        let composerInput = app.textViews["composer.input"]
        XCTAssertTrue(composerInput.waitForExistence(timeout: 5))
        composerInput.tap()
        composerInput.typeText("quick ui test")

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
        let hasSidebarSearch = app.textFields["sidebar.search"].exists
        let hasSidebarEmptyState = app.staticTexts["No threads"].exists || app.staticTexts["No matching threads"].exists
        XCTAssertTrue(hasSidebarSearch || hasSidebarEmptyState)

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
