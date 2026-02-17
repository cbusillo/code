import Foundation

enum ComposerSlashCommandActionID: String, Hashable {
    case insertPlanTemplate
    case insertStatusPrompt
    case insertTestPrompt
    case insertReviewPrompt
    case newThread
    case refreshThreads
    case openSettings
}

struct ComposerSlashCommand: Identifiable, Hashable {
    let command: String
    let title: String
    let summary: String
    let actionID: ComposerSlashCommandActionID

    var id: String {
        command
    }
}

enum ComposerSlashCommandCatalog {
    static let coreSet: [ComposerSlashCommand] = [
        ComposerSlashCommand(
            command: "/plan",
            title: "Plan",
            summary: "Draft a concise implementation plan with risks and validation.",
            actionID: .insertPlanTemplate
        ),
        ComposerSlashCommand(
            command: "/status",
            title: "Status",
            summary: "Summarize progress, blockers, and next step.",
            actionID: .insertStatusPrompt
        ),
        ComposerSlashCommand(
            command: "/test",
            title: "Test",
            summary: "Run relevant tests and summarize outcomes.",
            actionID: .insertTestPrompt
        ),
        ComposerSlashCommand(
            command: "/review",
            title: "Review",
            summary: "Review changes for bugs, regressions, and missing tests.",
            actionID: .insertReviewPrompt
        ),
        ComposerSlashCommand(
            command: "/new",
            title: "New thread",
            summary: "Create a new thread.",
            actionID: .newThread
        ),
        ComposerSlashCommand(
            command: "/refresh",
            title: "Refresh",
            summary: "Refresh thread list from the mirror server.",
            actionID: .refreshThreads
        ),
        ComposerSlashCommand(
            command: "/settings",
            title: "Settings",
            summary: "Open settings.",
            actionID: .openSettings
        )
    ]

    static func filteredCoreSet(query: String) -> [ComposerSlashCommand] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return coreSet
        }

        let normalized = trimmed.lowercased()
        return coreSet.filter { command in
            command.command.lowercased().contains(normalized)
                || command.title.lowercased().contains(normalized)
                || command.summary.lowercased().contains(normalized)
        }
    }
}

struct ComposerContextMentionMatch: Hashable {
    let query: String
    let range: Range<String.Index>
}

enum ComposerContextReferenceFormatter {
    private static let mentionPattern = "(?:^|\\s)@([A-Za-z0-9_./-]*)$"

    static func trailingMentionMatch(in draft: String) -> ComposerContextMentionMatch? {
        guard let regex = try? NSRegularExpression(pattern: mentionPattern) else {
            return nil
        }

        let nsRange = NSRange(draft.startIndex..<draft.endIndex, in: draft)
        guard let match = regex.firstMatch(in: draft, options: [], range: nsRange),
              let fullRange = Range(match.range, in: draft),
              let queryRange = Range(match.range(at: 1), in: draft)
        else {
            return nil
        }

        guard queryRange.lowerBound > fullRange.lowerBound else {
            return nil
        }

        let replacementStart = draft.index(before: queryRange.lowerBound)

        return ComposerContextMentionMatch(
            query: String(draft[queryRange]),
            range: replacementStart..<fullRange.upperBound
        )
    }

    static func formattedReferenceToken(path: String) -> String {
        if path.contains(" ") {
            return "@\"\(path)\""
        }
        return "@\(path)"
    }

    static func insertReference(
        into draft: String,
        path: String,
        mentionMatch: ComposerContextMentionMatch?
    ) -> String {
        let token = "\(formattedReferenceToken(path: path)) "
        guard let mentionMatch else {
            if draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                return token
            }

            if draft.hasSuffix("\n") || draft.hasSuffix(" ") {
                return "\(draft)\(token)"
            }
            return "\(draft) \(token)"
        }

        var updated = draft
        updated.replaceSubrange(mentionMatch.range, with: token)
        return updated
    }
}

func buildContextFileIndex(rootPath: String, maxCount: Int = 5_000) -> [String] {
    let rootURL = URL(fileURLWithPath: rootPath)
    let keys: Set<URLResourceKey> = [.isRegularFileKey, .isDirectoryKey]
    let skippedDirectoryNames: Set<String> = [
        ".git",
        ".code",
        ".build",
        "node_modules",
        "target",
        "build",
        "DerivedData"
    ]

    guard let enumerator = FileManager.default.enumerator(
        at: rootURL,
        includingPropertiesForKeys: Array(keys),
        options: [.skipsHiddenFiles, .skipsPackageDescendants]
    ) else {
        return []
    }

    var files: [String] = []
    files.reserveCapacity(min(maxCount, 1_024))

    while let object = enumerator.nextObject() as? URL {
        if files.count >= maxCount {
            break
        }

        let values = try? object.resourceValues(forKeys: keys)
        if values?.isDirectory == true,
           skippedDirectoryNames.contains(object.lastPathComponent) {
            enumerator.skipDescendants()
            continue
        }

        guard values?.isRegularFile == true else {
            continue
        }

        let absolute = object.path
        let rootPrefix = rootURL.path.hasSuffix("/") ? rootURL.path : "\(rootURL.path)/"
        let relative = absolute.hasPrefix(rootPrefix)
            ? String(absolute.dropFirst(rootPrefix.count))
            : object.lastPathComponent
        files.append(relative)
    }

    return files.sorted { lhs, rhs in
        lhs.localizedCaseInsensitiveCompare(rhs) == .orderedAscending
    }
}
