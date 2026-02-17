import Foundation

struct DiffRecoveryPlan: Hashable {
    let changedPaths: [String]
    let reviewCommand: String
    let snapshotCommand: String
    let restoreCommand: String
    let applySnapshotCommand: String

    init?(unifiedDiff: String) {
        let changedPaths = Self.extractChangedPaths(from: unifiedDiff)
        guard !changedPaths.isEmpty else {
            return nil
        }

        let quotedPaths = changedPaths.map(Self.shellQuote).joined(separator: " ")
        reviewCommand = "git diff -- \(quotedPaths)"
        snapshotCommand = "mkdir -p .code/snapshots && git diff -- \(quotedPaths) > .code/snapshots/recovery.patch"
        restoreCommand = "git restore --source=HEAD -- \(quotedPaths)"
        applySnapshotCommand = "git apply .code/snapshots/recovery.patch"
        self.changedPaths = changedPaths
    }

    private static func extractChangedPaths(from diff: String) -> [String] {
        var ordered: [String] = []
        var seen = Set<String>()

        for line in diff.components(separatedBy: "\n") {
            guard let path = extractPath(fromDiffHeader: line),
                  !path.isEmpty,
                  !seen.contains(path)
            else {
                continue
            }

            seen.insert(path)
            ordered.append(path)
        }

        return ordered
    }

    private static func extractPath(fromDiffHeader line: String) -> String? {
        guard line.hasPrefix("diff --git ") else {
            return nil
        }

        let remainder = String(line.dropFirst("diff --git ".count))
        let paths: (String, String)
        if let captured = capturePaths(
            in: remainder,
            pattern: #"^"a/(.+)" "b/(.+)"$"#
        ) {
            paths = captured
        } else if let captured = capturePaths(
            in: remainder,
            pattern: #"^a/(.+) b/(.+)$"#
        ) {
            paths = captured
        } else {
            return nil
        }

        let aPath = normalizeHeaderPath(paths.0)
        let bPath = normalizeHeaderPath(paths.1)

        if let bPath,
           bPath != "/dev/null" {
            return bPath
        }

        if let aPath,
           aPath != "/dev/null" {
            return aPath
        }

        return nil
    }

    private static func capturePaths(in value: String, pattern: String) -> (String, String)? {
        guard let regex = try? NSRegularExpression(pattern: pattern) else {
            return nil
        }

        let range = NSRange(value.startIndex..<value.endIndex, in: value)
        guard let match = regex.firstMatch(in: value, options: [], range: range),
              match.numberOfRanges == 3,
              let aRange = Range(match.range(at: 1), in: value),
              let bRange = Range(match.range(at: 2), in: value)
        else {
            return nil
        }

        return (String(value[aRange]), String(value[bRange]))
    }

    private static func normalizeHeaderPath(_ raw: String) -> String? {
        var value = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else {
            return nil
        }

        value = value
            .replacingOccurrences(of: "\\\"", with: "\"")
            .replacingOccurrences(of: "\\\\", with: "\\")
            .trimmingCharacters(in: .whitespacesAndNewlines)

        return value.isEmpty ? nil : value
    }

    private static func shellQuote(_ value: String) -> String {
        let escaped = value.replacingOccurrences(of: "'", with: "'\"'\"'")
        return "'\(escaped)'"
    }
}
