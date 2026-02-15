import Foundation

enum SessionHiddenReason: Equatable {
    case autoReviewWorktree
    case automationPrompt
    case harnessReviewPrompt
}

enum SessionVisibility {
    private static let automationPromptPrefixes: [String] = [
        "<user_action>",
        "context:",
        "strict review:",
        "you are a strict code reviewer",
        "you are a pr reviewer",
        "reply with exactly "
    ]

    static func hiddenReason(for session: SessionSummary) -> SessionHiddenReason? {
        if isAutoReviewWorktree(session.cwd) {
            return .autoReviewWorktree
        }

        if isLikelyAutomationPromptTitle(session.title) {
            return .automationPrompt
        }

        if isLikelyHarnessReviewPrompt(session.title) {
            return .harnessReviewPrompt
        }

        return nil
    }

    static func isHidden(_ session: SessionSummary) -> Bool {
        hiddenReason(for: session) != nil
    }

    private static func isAutoReviewWorktree(_ cwd: String) -> Bool {
        let normalized = cwd.lowercased()
        return normalized.contains("/.code/working/") && normalized.contains("/branches/auto-review")
    }

    private static func isLikelyAutomationPromptTitle(_ rawTitle: String?) -> Bool {
        let normalizedTitle = normalizeTitle(rawTitle)
        guard !normalizedTitle.isEmpty else {
            return false
        }

        if automationPromptPrefixes.contains(where: normalizedTitle.hasPrefix) {
            return true
        }

        if looksLikeUUID(normalizedTitle) {
            return true
        }

        if normalizedTitle.contains(" diff --git ") {
            return true
        }

        if normalizedTitle.hasPrefix("review ") && normalizedTitle.contains(" only.") {
            return true
        }

        return false
    }

    private static func isLikelyHarnessReviewPrompt(_ rawTitle: String?) -> Bool {
        let normalizedTitle = normalizeTitle(rawTitle)
        guard !normalizedTitle.isEmpty else {
            return false
        }

        if normalizedTitle.contains("every code harness") {
            return true
        }

        if normalizedTitle.contains("[running in read-only mode") {
            return true
        }

        if normalizedTitle.hasPrefix("review "),
           normalizedTitle.contains("files to consider:"),
           normalizedTitle.contains("output:") {
            return true
        }

        return false
    }

    private static func normalizeTitle(_ rawTitle: String?) -> String {
        guard let rawTitle else {
            return ""
        }

        return rawTitle
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: "\n", with: " ")
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
            .lowercased()
    }

    private static func looksLikeUUID(_ value: String) -> Bool {
        value.count == 36 && UUID(uuidString: value.uppercased()) != nil
    }
}
