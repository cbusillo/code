import Foundation

enum WorkflowSettings {
    static let defaultModelIdentifier = "gpt-5.3-codex"
    static let defaultModelLabel = "GPT-5.3-Codex"
    static let defaultReasoningEffort = "high"
    static let defaultReasoningLabel = "High"

    static let fallbackReasoningEfforts = ["minimal", "low", "medium", "high", "xhigh"]
    static let sandboxOptions = ["Local", "Workspace write", "Read-only"]
    static let approvalPolicyOptions = ["On request", "On failure", "Never"]

    static func canonicalModelIdentifier(from selection: String) -> String? {
        let trimmed = selection.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return nil
        }

        switch trimmed.lowercased() {
        case "gpt-5.3-codex":
            return "gpt-5.3-codex"
        case "gpt-5.2":
            return "gpt-5.2"
        case "claude sonnet 4.5", "claude-sonnet-4.5":
            return "claude-sonnet-4.5"
        default:
            return trimmed.contains(" ") ? nil : trimmed
        }
    }

    static func canonicalReasoningEffort(from selection: String) -> String? {
        let normalized = selection
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
            .replacingOccurrences(of: "_", with: "")
            .replacingOccurrences(of: "-", with: "")
            .replacingOccurrences(of: " ", with: "")

        guard !normalized.isEmpty else {
            return nil
        }

        switch normalized {
        case "none", "minimal":
            return "minimal"
        case "low":
            return "low"
        case "medium":
            return "medium"
        case "high":
            return "high"
        case "xhigh":
            return "xhigh"
        default:
            return nil
        }
    }

    static func displayReasoningEffort(_ effort: String) -> String {
        switch canonicalReasoningEffort(from: effort) {
        case "minimal":
            return "Minimal"
        case "low":
            return "Low"
        case "medium":
            return "Medium"
        case "high":
            return "High"
        case "xhigh":
            return "X-High"
        default:
            return effort
        }
    }

    static func normalizedSelection(
        current: String,
        options: [String],
        fallback: String
    ) -> String {
        let normalizedOptions = Set(options.map { $0.lowercased() })
        if normalizedOptions.contains(current.lowercased()) {
            if let exact = options.first(where: { $0.caseInsensitiveCompare(current) == .orderedSame }) {
                return exact
            }
        }

        return options.contains(fallback) ? fallback : (options.first ?? fallback)
    }
}
