import Foundation

enum WorkflowSettings {
    static let modelOptions = ["GPT-5.3-Codex", "GPT-5.2", "Claude Sonnet 4.5"]
    static let reasoningOptions = ["Low", "Medium", "High"]
    static let sandboxOptions = ["Local", "Workspace write", "Read-only"]
    static let approvalPolicyOptions = ["On request", "On failure", "Never"]

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
