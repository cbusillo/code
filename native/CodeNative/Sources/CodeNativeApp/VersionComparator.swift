import Foundation

enum VersionComparator {
    static func compare(_ lhs: String, _ rhs: String) -> ComparisonResult {
        let lhsParts = numericVersionComponents(from: lhs)
        let rhsParts = numericVersionComponents(from: rhs)
        let count = max(lhsParts.count, rhsParts.count)

        for index in 0..<count {
            let lhsPart = index < lhsParts.count ? lhsParts[index] : 0
            let rhsPart = index < rhsParts.count ? rhsParts[index] : 0
            if lhsPart < rhsPart {
                return .orderedAscending
            }
            if lhsPart > rhsPart {
                return .orderedDescending
            }
        }

        return .orderedSame
    }

    static func isAtLeast(_ lhs: String, minimum rhs: String) -> Bool {
        compare(lhs, rhs) != .orderedAscending
    }

    static func normalizedVersionString(_ raw: String?) -> String? {
        guard let raw else {
            return nil
        }

        let normalized = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return normalized.isEmpty ? nil : normalized
    }

    private static func numericVersionComponents(from raw: String) -> [Int] {
        raw.split(whereSeparator: { !$0.isNumber }).compactMap { Int($0) }
    }
}

