import Foundation

enum ComposerSlashCommandActionID: Hashable {
    case insertCommand(String)
    case newThread
    case refreshThreads
    case openSettings
    case openContextPicker
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
            summary: "Draft a comprehensive implementation plan using agents.",
            actionID: .insertCommand("/plan ")
        ),
        ComposerSlashCommand(
            command: "/code",
            title: "Code",
            summary: "Run a coding task with the multi-agent coding workflow.",
            actionID: .insertCommand("/code ")
        ),
        ComposerSlashCommand(
            command: "/solve",
            title: "Solve",
            summary: "Deep-dive a hard problem with multi-agent analysis.",
            actionID: .insertCommand("/solve ")
        ),
        ComposerSlashCommand(
            command: "/review",
            title: "Review",
            summary: "Run review mode to find concrete bugs and regressions.",
            actionID: .insertCommand("/review ")
        ),
        ComposerSlashCommand(
            command: "/status",
            title: "Status",
            summary: "Show session configuration, usage, and runtime context.",
            actionID: .insertCommand("/status")
        ),
        ComposerSlashCommand(
            command: "/test",
            title: "Test",
            summary: "Run relevant tests and report pass/fail evidence.",
            actionID: .insertCommand("/test")
        ),
        ComposerSlashCommand(
            command: "/diff",
            title: "Diff",
            summary: "Inspect current workspace diff including untracked files.",
            actionID: .insertCommand("/diff")
        ),
        ComposerSlashCommand(
            command: "/undo",
            title: "Undo",
            summary: "Open snapshot recovery flow to restore workspace state.",
            actionID: .insertCommand("/undo")
        ),
        ComposerSlashCommand(
            command: "/mention",
            title: "Mention",
            summary: "Insert @context references from workspace files.",
            actionID: .openContextPicker
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
        let normalizedWithoutPrefix = normalized.hasPrefix("/") ? String(normalized.dropFirst()) : normalized
        return coreSet.filter { command in
            command.command.lowercased().contains(normalized)
                || command.command.dropFirst().lowercased().contains(normalizedWithoutPrefix)
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
