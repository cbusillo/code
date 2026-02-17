import Foundation

enum SessionRailGroupingMode: String, CaseIterable, Identifiable {
    case repository
    case flat

    var id: String { rawValue }

    var label: String {
        switch self {
        case .repository:
            return "Repository"
        case .flat:
            return "Flat list"
        }
    }
}

struct SessionRailGroup: Identifiable, Equatable {
    var id: String { key }

    let key: String
    let title: String
    let sessions: [SessionSummary]
    let totalCount: Int
    let latestActivityUnixMs: UInt64

    var hiddenCount: Int {
        max(totalCount - sessions.count, 0)
    }
}

struct SessionRailLayout: Equatable {
    let groups: [SessionRailGroup]
    let totalCount: Int
    let visibleCount: Int

    var hiddenCount: Int {
        max(totalCount - visibleCount, 0)
    }

    var isTruncated: Bool {
        hiddenCount > 0
    }
}

enum SessionRailDisplayPolicy {
    static let minVisibleLimit = 20
    static let defaultVisibleLimit = 120
    static let visibleLimitStep = 100
    static let maxVisibleLimit = 2_000

    static func normalizedVisibleLimit(_ raw: Int) -> Int {
        min(max(raw, minVisibleLimit), maxVisibleLimit)
    }
}

func sessionRailRepoName(for session: SessionSummary) -> String {
    let repoName = URL(fileURLWithPath: session.cwd).lastPathComponent
    return repoName.isEmpty ? "workspace" : repoName
}

func sessionRailActivityUnixMs(_ session: SessionSummary) -> UInt64 {
    max(session.lastEventAtUnixMs, session.createdAtUnixMs)
}

func buildSessionRailLayout(
    sessions: [SessionSummary],
    groupingMode: SessionRailGroupingMode,
    selectedSessionID: UUID?,
    visibleLimit: Int
) -> SessionRailLayout {
    let sortedSessions = sessions.sorted(by: {
        let lhsActivity = sessionRailActivityUnixMs($0)
        let rhsActivity = sessionRailActivityUnixMs($1)
        if lhsActivity == rhsActivity {
            return $0.id.uuidString < $1.id.uuidString
        }
        return lhsActivity > rhsActivity
    })

    let totalCount = sortedSessions.count
    guard totalCount > 0 else {
        return SessionRailLayout(groups: [], totalCount: 0, visibleCount: 0)
    }

    let normalizedLimit = SessionRailDisplayPolicy.normalizedVisibleLimit(visibleLimit)
    var visibleSessions = Array(sortedSessions.prefix(normalizedLimit))

    if let selectedSessionID,
       !visibleSessions.contains(where: { $0.id == selectedSessionID }),
       let selectedSession = sortedSessions.first(where: { $0.id == selectedSessionID }) {
        if visibleSessions.isEmpty {
            visibleSessions = [selectedSession]
        } else {
            visibleSessions[visibleSessions.count - 1] = selectedSession
            visibleSessions = visibleSessions.sorted(by: {
                let lhsActivity = sessionRailActivityUnixMs($0)
                let rhsActivity = sessionRailActivityUnixMs($1)
                if lhsActivity == rhsActivity {
                    return $0.id.uuidString < $1.id.uuidString
                }
                return lhsActivity > rhsActivity
            })
        }
    }

    let visibleCount = visibleSessions.count
    let totalCountsByRepo = sortedSessions.reduce(into: [String: Int]()) { partialResult, session in
        partialResult[sessionRailRepoName(for: session), default: 0] += 1
    }

    switch groupingMode {
    case .flat:
        let group = SessionRailGroup(
            key: "all",
            title: "All threads",
            sessions: visibleSessions,
            totalCount: totalCount,
            latestActivityUnixMs: visibleSessions.first.map(sessionRailActivityUnixMs) ?? 0
        )
        return SessionRailLayout(groups: [group], totalCount: totalCount, visibleCount: visibleCount)

    case .repository:
        let grouped = Dictionary(grouping: visibleSessions, by: sessionRailRepoName(for:))
        let groups = grouped
            .map { repoName, sessions in
                let sortedRepoSessions = sessions.sorted(by: {
                    let lhsActivity = sessionRailActivityUnixMs($0)
                    let rhsActivity = sessionRailActivityUnixMs($1)
                    if lhsActivity == rhsActivity {
                        return $0.id.uuidString < $1.id.uuidString
                    }
                    return lhsActivity > rhsActivity
                })
                let latestActivity = sortedRepoSessions.first.map(sessionRailActivityUnixMs) ?? 0
                return SessionRailGroup(
                    key: repoName,
                    title: repoName,
                    sessions: sortedRepoSessions,
                    totalCount: totalCountsByRepo[repoName, default: sortedRepoSessions.count],
                    latestActivityUnixMs: latestActivity
                )
            }
            .sorted(by: {
                if $0.latestActivityUnixMs == $1.latestActivityUnixMs {
                    return $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending
                }
                return $0.latestActivityUnixMs > $1.latestActivityUnixMs
            })

        return SessionRailLayout(groups: groups, totalCount: totalCount, visibleCount: visibleCount)
    }
}
