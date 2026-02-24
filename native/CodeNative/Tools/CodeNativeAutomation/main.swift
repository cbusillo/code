import AppKit
import ApplicationServices
import Foundation

struct ScenarioStep: Decodable {
    let kind: String
    let id: String?
    let title: String?
    let text: String?
    let seconds: Double?
    let path: String?
    let maxDepth: Int?
}

enum AutomationError: Error, CustomStringConvertible {
    case usage(String)
    case scenarioReadFailed(String)
    case scenarioDecodeFailed(String)
    case accessibilityDenied
    case appNotRunning(String)
    case elementNotFound(String)
    case actionFailed(String)

    var description: String {
        switch self {
        case .usage(let message):
            return message
        case .scenarioReadFailed(let message):
            return "Failed to read scenario: \(message)"
        case .scenarioDecodeFailed(let message):
            return "Failed to decode scenario JSON: \(message)"
        case .accessibilityDenied:
            return "Accessibility access is required. Grant Terminal/Code access in macOS Settings."
        case .appNotRunning(let appName):
            return "App '\(appName)' is not running. Start it first (for example: pnpm dev:native)."
        case .elementNotFound(let identifier):
            return "No accessibility element found for identifier '\(identifier)'."
        case .actionFailed(let message):
            return message
        }
    }
}

struct CliOptions {
    var appName = "CodeNativeApp"
    var scenarioPath: String?
    var activate = true
}

private func parseOptions(_ argv: [String]) throws -> CliOptions {
    var options = CliOptions()
    var index = 0

    while index < argv.count {
        let arg = argv[index]
        switch arg {
        case "--app":
            index += 1
            guard index < argv.count else {
                throw AutomationError.usage("--app requires a value")
            }
            options.appName = argv[index]
        case "--scenario":
            index += 1
            guard index < argv.count else {
                throw AutomationError.usage("--scenario requires a value")
            }
            options.scenarioPath = argv[index]
        case "--no-activate":
            options.activate = false
        case "-h", "--help":
            throw AutomationError.usage(usageText)
        default:
            throw AutomationError.usage("Unknown arg: \(arg)\n\n\(usageText)")
        }
        index += 1
    }

    guard options.scenarioPath != nil else {
        throw AutomationError.usage("--scenario is required\n\n\(usageText)")
    }

    return options
}

private let usageText = """
Usage:
  swift run --package-path native/CodeNative CodeNativeAutomation --scenario <path> [--app CodeNativeApp] [--no-activate]

Scenario format:
  JSON array of steps, each with a 'kind':
    wait      { "kind": "wait", "seconds": 0.4 }
    click     { "kind": "click", "id": "composer.send" }
    click     { "kind": "click", "title": "New thread" }
    type      { "kind": "type", "id": "composer.input", "text": "hello" }
    screenshot{ "kind": "screenshot", "path": "docs/reference/native-ui/shot.png" }
    dump_tree { "kind": "dump_tree", "maxDepth": 3 }
"""

private func loadScenario(path: String) throws -> [ScenarioStep] {
    let url = URL(fileURLWithPath: path)
    let data: Data
    do {
        data = try Data(contentsOf: url)
    } catch {
        throw AutomationError.scenarioReadFailed(error.localizedDescription)
    }

    do {
        return try JSONDecoder().decode([ScenarioStep].self, from: data)
    } catch {
        throw AutomationError.scenarioDecodeFailed(error.localizedDescription)
    }
}

private func ensureAccessibilityTrust() throws {
    let options = ["AXTrustedCheckOptionPrompt": true] as CFDictionary
    guard AXIsProcessTrustedWithOptions(options) else {
        throw AutomationError.accessibilityDenied
    }
}

private func matchingApplications(named appName: String) -> [NSRunningApplication] {
    NSWorkspace.shared.runningApplications.filter {
        $0.localizedName == appName || $0.bundleIdentifier == appName
    }
}

private func appHasVisibleWindow(_ app: NSRunningApplication) -> Bool {
    let element = AXUIElementCreateApplication(app.processIdentifier)
    if let focusedWindow = copyAttribute(element, kAXFocusedWindowAttribute as CFString),
       CFGetTypeID(focusedWindow) == AXUIElementGetTypeID() {
        return true
    }

    if let windows = copyAttribute(element, kAXWindowsAttribute as CFString) as? [AnyObject],
       !windows.isEmpty {
        return true
    }

    return false
}

private func selectApplication(named appName: String) -> NSRunningApplication? {
    let matches = matchingApplications(named: appName)
    if matches.isEmpty {
        return nil
    }

    if let frontmostWithWindow = matches.first(where: { $0.isActive && appHasVisibleWindow($0) }) {
        return frontmostWithWindow
    }

    if let anyWithWindow = matches.first(where: appHasVisibleWindow) {
        return anyWithWindow
    }

    return matches.first
}

private func copyAttribute(_ element: AXUIElement, _ attribute: CFString) -> AnyObject? {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, attribute, &value)
    guard result == .success else {
        return nil
    }
    return value as AnyObject?
}

private func boolAttribute(_ element: AXUIElement, _ attribute: CFString) -> Bool {
    guard let value = copyAttribute(element, attribute) else {
        return false
    }
    if CFGetTypeID(value) == CFBooleanGetTypeID() {
        return CFBooleanGetValue((value as! CFBoolean))
    }
    return false
}

private func stringAttribute(_ element: AXUIElement, _ attribute: CFString) -> String? {
    copyAttribute(element, attribute) as? String
}

private func displayLabel(for element: AXUIElement) -> String {
    let role = stringAttribute(element, kAXRoleAttribute as CFString) ?? "-"
    let title = stringAttribute(element, kAXTitleAttribute as CFString) ?? ""
    let identifier = stringAttribute(element, kAXIdentifierAttribute as CFString) ?? ""
    if !identifier.isEmpty {
        return "role=\(role) title=\(title) id=\(identifier)"
    }
    if !title.isEmpty {
        return "role=\(role) title=\(title)"
    }
    return "role=\(role)"
}

private func children(of element: AXUIElement) -> [AXUIElement] {
    let childAttributes: [CFString] = [
        kAXChildrenAttribute as CFString,
        kAXVisibleChildrenAttribute as CFString,
        kAXWindowsAttribute as CFString,
        kAXRowsAttribute as CFString,
        kAXTabsAttribute as CFString,
    ]

    var result: [AXUIElement] = []
    result.reserveCapacity(16)

    for attribute in childAttributes {
        guard let value = copyAttribute(element, attribute) else {
            continue
        }

        if CFGetTypeID(value) == AXUIElementGetTypeID() {
            result.append(unsafeDowncast(value, to: AXUIElement.self))
            continue
        }

        if let values = value as? [AnyObject] {
            for item in values where CFGetTypeID(item) == AXUIElementGetTypeID() {
                result.append(unsafeDowncast(item, to: AXUIElement.self))
            }
        }
    }

    return result
}

private func findElement(root: AXUIElement, identifier: String, maxDepth: Int = 80) -> AXUIElement? {
    var queue: [(AXUIElement, Int)] = [(root, 0)]
    var cursor = 0
    var visited: Set<UnsafeMutableRawPointer> = []
    var visitedCount = 0
    let maxVisited = 12_000

    while cursor < queue.count {
        let (element, depth) = queue[cursor]
        cursor += 1
        let address = Unmanaged.passUnretained(element).toOpaque()
        if visited.contains(address) {
            continue
        }
        visited.insert(address)
        visitedCount += 1
        if visitedCount > maxVisited {
            break
        }

        if let elementIdentifier = stringAttribute(element, kAXIdentifierAttribute as CFString),
           elementIdentifier == identifier {
            return element
        }
        if depth >= maxDepth {
            continue
        }
        for child in children(of: element) {
            queue.append((child, depth + 1))
        }
    }

    return nil
}

private func findElementByTitle(root: AXUIElement, title: String, maxDepth: Int = 80) -> AXUIElement? {
    var queue: [(AXUIElement, Int)] = [(root, 0)]
    var cursor = 0
    var visited: Set<UnsafeMutableRawPointer> = []
    var visitedCount = 0
    let maxVisited = 12_000

    while cursor < queue.count {
        let (element, depth) = queue[cursor]
        cursor += 1
        let address = Unmanaged.passUnretained(element).toOpaque()
        if visited.contains(address) {
            continue
        }
        visited.insert(address)
        visitedCount += 1
        if visitedCount > maxVisited {
            break
        }

        if stringAttribute(element, kAXTitleAttribute as CFString) == title {
            return element
        }
        if depth >= maxDepth {
            continue
        }
        for child in children(of: element) {
            queue.append((child, depth + 1))
        }
    }

    return nil
}

private func dumpTree(root: AXUIElement, maxDepth: Int) {
    var queue: [(AXUIElement, Int)] = [(root, 0)]
    var cursor = 0
    var visited: Set<UnsafeMutableRawPointer> = []

    while cursor < queue.count {
        let (element, depth) = queue[cursor]
        cursor += 1
        let address = Unmanaged.passUnretained(element).toOpaque()
        if visited.contains(address) {
            continue
        }
        visited.insert(address)

        let indent = String(repeating: "  ", count: depth)
        print("\(indent)- \(displayLabel(for: element))")

        if depth >= maxDepth {
            continue
        }

        for child in children(of: element) {
            queue.append((child, depth + 1))
        }
    }
}

private func setFocus(on element: AXUIElement) {
    _ = AXUIElementSetAttributeValue(
        element,
        kAXFocusedAttribute as CFString,
        kCFBooleanTrue
    )
}

private func click(_ element: AXUIElement) throws {
    let press = AXUIElementPerformAction(element, kAXPressAction as CFString)
    if press == .success {
        return
    }
    let showMenu = AXUIElementPerformAction(element, kAXShowMenuAction as CFString)
    if showMenu == .success {
        return
    }
    throw AutomationError.actionFailed("Failed to click element.")
}

private func postKeyboardShortcut(
    virtualKey: CGKeyCode,
    flags: CGEventFlags,
    appPid: pid_t
) throws {
    guard let source = CGEventSource(stateID: .hidSystemState),
          let keyDown = CGEvent(keyboardEventSource: source, virtualKey: virtualKey, keyDown: true),
          let keyUp = CGEvent(keyboardEventSource: source, virtualKey: virtualKey, keyDown: false)
    else {
        throw AutomationError.actionFailed("Failed to create keyboard events for shortcut input.")
    }

    keyDown.flags = flags
    keyUp.flags = flags
    keyDown.postToPid(appPid)
    keyUp.postToPid(appPid)
}

private func postUnicodeText(_ text: String, appPid: pid_t) throws {
    guard !text.isEmpty else {
        return
    }

    guard let source = CGEventSource(stateID: .hidSystemState) else {
        throw AutomationError.actionFailed("Failed to create keyboard event source for typing.")
    }

    let utf16 = Array(text.utf16)
    guard let keyDown = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true),
          let keyUp = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false)
    else {
        throw AutomationError.actionFailed("Failed to create keyboard events for typing.")
    }

    keyDown.keyboardSetUnicodeString(stringLength: utf16.count, unicodeString: utf16)
    keyUp.keyboardSetUnicodeString(stringLength: utf16.count, unicodeString: utf16)
    keyDown.postToPid(appPid)
    keyUp.postToPid(appPid)
}

private func typeText(_ text: String, into element: AXUIElement, appPid: pid_t) throws {
    setFocus(on: element)
    Thread.sleep(forTimeInterval: 0.05)

    // Clear existing text via Cmd+A then Delete to keep scenarios deterministic.
    try postKeyboardShortcut(virtualKey: 0x00, flags: .maskCommand, appPid: appPid)
    try postKeyboardShortcut(virtualKey: 0x33, flags: [], appPid: appPid)
    Thread.sleep(forTimeInterval: 0.03)
    try postUnicodeText(text, appPid: appPid)
}

private func focusedWindow(for appElement: AXUIElement) -> AXUIElement? {
    if let value = copyAttribute(appElement, kAXFocusedWindowAttribute as CFString) {
        return unsafeDowncast(value, to: AXUIElement.self)
    }
    if let values = copyAttribute(appElement, kAXWindowsAttribute as CFString) as? [AXUIElement] {
        return values.first
    }
    return nil
}

private func cgWindowID(for window: AXUIElement) -> CGWindowID? {
    let windowNumberKey = "AXWindowNumber" as CFString
    guard let value = copyAttribute(window, windowNumberKey) else {
        return nil
    }

    if let number = value as? NSNumber {
        return CGWindowID(number.uint32Value)
    }

    return nil
}

private func cgWindowID(forAppPid pid: pid_t) -> CGWindowID? {
    guard let windowInfoList = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements],
        kCGNullWindowID
    ) as? [[String: Any]]
    else {
        return nil
    }

    var bestWindowID: CGWindowID?
    var bestArea: CGFloat = 0

    for info in windowInfoList {
        guard let ownerPid = info[kCGWindowOwnerPID as String] as? NSNumber,
              ownerPid.int32Value == pid,
              let layer = info[kCGWindowLayer as String] as? NSNumber,
              layer.intValue == 0,
              let boundsDict = info[kCGWindowBounds as String] as? [String: Any],
              let bounds = CGRect(dictionaryRepresentation: boundsDict as CFDictionary),
              bounds.width > 0,
              bounds.height > 0,
              let windowNumber = info[kCGWindowNumber as String] as? NSNumber
        else {
            continue
        }

        let area = bounds.width * bounds.height
        if area > bestArea {
            bestArea = area
            bestWindowID = CGWindowID(windowNumber.uint32Value)
        }
    }

    return bestWindowID
}

private func cgPoint(from value: AXValue) -> CGPoint? {
    var point = CGPoint.zero
    if AXValueGetType(value) == .cgPoint,
       AXValueGetValue(value, .cgPoint, &point) {
        return point
    }
    return nil
}

private func cgSize(from value: AXValue) -> CGSize? {
    var size = CGSize.zero
    if AXValueGetType(value) == .cgSize,
       AXValueGetValue(value, .cgSize, &size) {
        return size
    }
    return nil
}

private func captureScreenshot(appElement: AXUIElement, appPid: pid_t, path: String) throws {
    guard let window = focusedWindow(for: appElement) else {
        throw AutomationError.actionFailed("Could not find focused window for screenshot.")
    }

    let outputUrl = URL(fileURLWithPath: path)
    let dir = outputUrl.deletingLastPathComponent()
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

    if let windowID = cgWindowID(forAppPid: appPid) ?? cgWindowID(for: window) {
        let windowProcess = Process()
        windowProcess.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        windowProcess.arguments = ["-x", "-l", String(windowID), outputUrl.path]
        try windowProcess.run()
        windowProcess.waitUntilExit()
        if windowProcess.terminationStatus == 0 {
            return
        }
    }

    guard let positionValue = copyAttribute(window, kAXPositionAttribute as CFString),
          let sizeValue = copyAttribute(window, kAXSizeAttribute as CFString),
          CFGetTypeID(positionValue) == AXValueGetTypeID(),
          CFGetTypeID(sizeValue) == AXValueGetTypeID()
    else {
        throw AutomationError.actionFailed("Could not resolve window bounds for screenshot.")
    }

    let positionAxValue = unsafeDowncast(positionValue, to: AXValue.self)
    let sizeAxValue = unsafeDowncast(sizeValue, to: AXValue.self)

    guard let position = cgPoint(from: positionAxValue),
          let size = cgSize(from: sizeAxValue)
    else {
        throw AutomationError.actionFailed("Could not decode window geometry values.")
    }

    let rectArg = "\(Int(position.x.rounded())),\(Int(position.y.rounded())),\(Int(size.width.rounded())),\(Int(size.height.rounded()))"

    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    process.arguments = ["-x", "-R\(rectArg)", outputUrl.path]

    try process.run()
    process.waitUntilExit()

    guard process.terminationStatus == 0 else {
        throw AutomationError.actionFailed("screencapture failed with status \(process.terminationStatus).")
    }
}

private func runScenario(
    _ steps: [ScenarioStep],
    appName: String,
    activate: Bool
) throws {
    try ensureAccessibilityTrust()

    guard let app = selectApplication(named: appName) else {
        throw AutomationError.appNotRunning(appName)
    }

    if activate {
        _ = app.activate()
        Thread.sleep(forTimeInterval: 0.2)
    }

    let appElement = AXUIElementCreateApplication(app.processIdentifier)
    AXUIElementSetMessagingTimeout(appElement, 0.2)

    for step in steps {
        switch step.kind {
        case "wait":
            Thread.sleep(forTimeInterval: step.seconds ?? 0.2)

        case "click":
            let element: AXUIElement?
            if let id = step.id {
                element = findElement(root: appElement, identifier: id)
            } else if let title = step.title {
                element = findElementByTitle(root: appElement, title: title)
            } else {
                throw AutomationError.actionFailed("click step requires 'id' or 'title'.")
            }

            guard let element else {
                if let id = step.id {
                    throw AutomationError.elementNotFound(id)
                }
                throw AutomationError.elementNotFound(step.title ?? "")
            }
            try click(element)

        case "type":
            guard let text = step.text else {
                throw AutomationError.actionFailed("type step requires 'text'.")
            }

            let element: AXUIElement?
            if let id = step.id {
                element = findElement(root: appElement, identifier: id)
            } else if let title = step.title {
                element = findElementByTitle(root: appElement, title: title)
            } else {
                throw AutomationError.actionFailed("type step requires 'id' or 'title'.")
            }

            guard let element else {
                if let id = step.id {
                    throw AutomationError.elementNotFound(id)
                }
                throw AutomationError.elementNotFound(step.title ?? "")
            }
            try typeText(text, into: element, appPid: app.processIdentifier)

        case "screenshot":
            guard let path = step.path else {
                throw AutomationError.actionFailed("screenshot step requires 'path'.")
            }
            try captureScreenshot(appElement: appElement, appPid: app.processIdentifier, path: path)

        case "dump_tree":
            dumpTree(root: appElement, maxDepth: step.maxDepth ?? 4)

        default:
            throw AutomationError.actionFailed("Unsupported step kind '\(step.kind)'.")
        }
    }
}

@main
struct CodeNativeAutomationMain {
    static func main() {
        var argv = Array(CommandLine.arguments.dropFirst())
        while argv.first == "--" {
            argv.removeFirst()
        }
        if argv.contains("--help") || argv.contains("-h") {
            print(usageText)
            exit(0)
        }

        do {
            let options = try parseOptions(argv)
            let scenario = try loadScenario(path: options.scenarioPath!)
            try runScenario(scenario, appName: options.appName, activate: options.activate)
            print("Scenario complete.")
        } catch let error as AutomationError {
            fputs("\(error.description)\n", stderr)
            exit(1)
        } catch {
            fputs("Unexpected error: \(error.localizedDescription)\n", stderr)
            exit(1)
        }
    }
}
