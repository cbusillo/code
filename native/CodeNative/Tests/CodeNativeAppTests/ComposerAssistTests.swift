import XCTest
@testable import CodeNativeApp

final class ComposerAssistTests: XCTestCase {
    func testSlashCommandCatalogFiltersCoreCommands() {
        let commands = ComposerSlashCommandCatalog.filteredCoreSet(query: "plan")
        XCTAssertEqual(commands.first?.command, "/plan")
        XCTAssertTrue(commands.contains(where: { $0.command == "/plan" }))
    }

    func testSlashCommandCatalogCoversHighFrequencyWorkflows() {
        let commands = ComposerSlashCommandCatalog.coreSet.map(\.command)
        XCTAssertTrue(commands.contains("/plan"))
        XCTAssertTrue(commands.contains("/code"))
        XCTAssertTrue(commands.contains("/solve"))
        XCTAssertTrue(commands.contains("/review"))
        XCTAssertTrue(commands.contains("/status"))
        XCTAssertTrue(commands.contains("/diff"))
        XCTAssertTrue(commands.contains("/undo"))
        XCTAssertTrue(commands.contains("/mention"))
    }

    func testSlashCommandCatalogFiltersLeadingSlashQueries() {
        let commands = ComposerSlashCommandCatalog.filteredCoreSet(query: "/sol")
        XCTAssertEqual(commands.first?.command, "/solve")
    }

    func testTrailingMentionMatchAndReplacement() {
        let draft = "Please inspect @native/Code"
        let mention = ComposerContextReferenceFormatter.trailingMentionMatch(in: draft)

        XCTAssertEqual(mention?.query, "native/Code")

        let updated = ComposerContextReferenceFormatter.insertReference(
            into: draft,
            path: "native/CodeNative/Sources/CodeNativeApp/ContentView.swift",
            mentionMatch: mention
        )

        XCTAssertEqual(
            updated,
            "Please inspect @native/CodeNative/Sources/CodeNativeApp/ContentView.swift "
        )
    }

    func testTrailingMentionMatchSupportsQuotedAndPunctuationTriggers() {
        let quotedDraft = "Please inspect (@\"docs/native app\""
        let quotedMatch = ComposerContextReferenceFormatter.trailingMentionMatch(in: quotedDraft)
        XCTAssertEqual(quotedMatch?.query, "docs/native app")

        let punctuatedDraft = "Need file: @native/CodeNative"
        let punctuatedMatch = ComposerContextReferenceFormatter.trailingMentionMatch(in: punctuatedDraft)
        XCTAssertEqual(punctuatedMatch?.query, "native/CodeNative")
    }

    func testInsertReferenceReplacesQuotedMentionRange() {
        let draft = "Inspect (@\"docs/native app\""
        let mention = ComposerContextReferenceFormatter.trailingMentionMatch(in: draft)

        let updated = ComposerContextReferenceFormatter.insertReference(
            into: draft,
            path: "docs/native app parity.md",
            mentionMatch: mention
        )

        XCTAssertEqual(updated, "Inspect (@\"docs/native app parity.md\" ")
    }

    func testFormattedReferenceTokenQuotesWhitespacePaths() {
        let token = ComposerContextReferenceFormatter.formattedReferenceToken(path: "docs/native app parity.md")
        XCTAssertEqual(token, "@\"docs/native app parity.md\"")
    }

    func testContextPathFilteringPrefersBasenameAndLimitsCount() {
        let indexed = [
            "docs/native-app-parity-plan.md",
            "native/CodeNative/Sources/CodeNativeApp/ContentView.swift",
            "native/CodeNative/Sources/CodeNativeApp/ComposerAssist.swift",
            "scripts/ux/benchmark-native-ui.sh"
        ]

        let filtered = ComposerContextPathCatalog.filteredPaths(
            from: indexed,
            query: "contentview",
            limit: 2
        )

        XCTAssertEqual(filtered.count, 1)
        XCTAssertEqual(filtered.first, "native/CodeNative/Sources/CodeNativeApp/ContentView.swift")
    }

    func testBuildContextFileIndexSkipsHiddenAndLargeDirectories() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("composer-context-index-\(UUID().uuidString)")

        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer {
            try? FileManager.default.removeItem(at: root)
        }

        let file = root.appendingPathComponent("src/main.swift")
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try "print(\"hello\")".write(to: file, atomically: true, encoding: .utf8)

        let gitIgnored = root.appendingPathComponent(".git/config")
        try FileManager.default.createDirectory(at: gitIgnored.deletingLastPathComponent(), withIntermediateDirectories: true)
        try "[core]".write(to: gitIgnored, atomically: true, encoding: .utf8)

        let indexed = buildContextFileIndex(rootPath: root.path)
        XCTAssertTrue(indexed.contains(where: { $0.hasSuffix("main.swift") }))
        XCTAssertFalse(indexed.contains(".git/config"))
    }
}
