import Foundation
import XCTest
@testable import CodeNativeApp

final class SessionRailLayoutTests: XCTestCase {
    func testBuildSessionRailLayoutGroupsByRepositoryAndSortsByLatestActivity() {
        let repoARecent = makeSession(
            idSuffix: "10000000-0000-0000-0000-000000000001",
            cwd: "/tmp/repo-a",
            activityUnixMs: 300,
            title: "A recent"
        )
        let repoAOld = makeSession(
            idSuffix: "10000000-0000-0000-0000-000000000002",
            cwd: "/tmp/repo-a",
            activityUnixMs: 120,
            title: "A old"
        )
        let repoB = makeSession(
            idSuffix: "10000000-0000-0000-0000-000000000003",
            cwd: "/tmp/repo-b",
            activityUnixMs: 250,
            title: "B"
        )

        let layout = buildSessionRailLayout(
            sessions: [repoAOld, repoB, repoARecent],
            groupingMode: .repository,
            selectedSessionID: nil,
            visibleLimit: 120
        )

        XCTAssertEqual(layout.totalCount, 3)
        XCTAssertEqual(layout.visibleCount, 3)
        XCTAssertEqual(layout.groups.map(\.title), ["repo-a", "repo-b"])
        XCTAssertEqual(layout.groups.first?.sessions.map(\.id), [repoARecent.id, repoAOld.id])
    }

    func testBuildSessionRailLayoutFlatModeProducesSingleGroup() {
        let first = makeSession(
            idSuffix: "20000000-0000-0000-0000-000000000001",
            cwd: "/tmp/repo-a",
            activityUnixMs: 100,
            title: "first"
        )
        let second = makeSession(
            idSuffix: "20000000-0000-0000-0000-000000000002",
            cwd: "/tmp/repo-b",
            activityUnixMs: 200,
            title: "second"
        )

        let layout = buildSessionRailLayout(
            sessions: [first, second],
            groupingMode: .flat,
            selectedSessionID: nil,
            visibleLimit: 120
        )

        XCTAssertEqual(layout.groups.count, 1)
        XCTAssertEqual(layout.groups[0].title, "All threads")
        XCTAssertEqual(layout.groups[0].sessions.map(\.id), [second.id, first.id])
    }

    func testBuildSessionRailLayoutIncludesSelectedSessionOutsideVisibleLimit() {
        let sessions = (0..<40).map { index in
            makeSession(
                idSuffix: String(format: "30000000-0000-0000-0000-%012d", index + 1),
                cwd: "/tmp/repo-\(index % 2)",
                activityUnixMs: UInt64(1_000 - index * 10),
                title: "session-\(index)"
            )
        }
        let selected = sessions.last!

        let layout = buildSessionRailLayout(
            sessions: sessions,
            groupingMode: .flat,
            selectedSessionID: selected.id,
            visibleLimit: SessionRailDisplayPolicy.minVisibleLimit
        )

        XCTAssertEqual(layout.visibleCount, SessionRailDisplayPolicy.minVisibleLimit)
        XCTAssertTrue(layout.groups[0].sessions.contains(where: { $0.id == selected.id }))
        XCTAssertEqual(layout.hiddenCount, 20)
    }

    func testBuildSessionRailLayoutTracksPerGroupHiddenCountsWhenTruncated() {
        let repoASessions = (0..<25).map { index in
            makeSession(
                idSuffix: String(format: "40000000-0000-0000-0000-%012d", index + 1),
                cwd: "/tmp/repo-a",
                activityUnixMs: UInt64(1_200 - index * 2),
                title: "repo-a-\(index)"
            )
        }
        let repoBSessions = (0..<15).map { index in
            makeSession(
                idSuffix: String(format: "50000000-0000-0000-0000-%012d", index + 1),
                cwd: "/tmp/repo-b",
                activityUnixMs: UInt64(1_199 - index * 2),
                title: "repo-b-\(index)"
            )
        }

        let layout = buildSessionRailLayout(
            sessions: repoASessions + repoBSessions,
            groupingMode: .repository,
            selectedSessionID: nil,
            visibleLimit: SessionRailDisplayPolicy.minVisibleLimit
        )

        let repoAGroup = layout.groups.first(where: { $0.title == "repo-a" })
        let repoBGroup = layout.groups.first(where: { $0.title == "repo-b" })

        XCTAssertEqual(layout.totalCount, 40)
        XCTAssertEqual(layout.visibleCount, SessionRailDisplayPolicy.minVisibleLimit)
        XCTAssertEqual(layout.hiddenCount, 20)
        XCTAssertEqual(repoAGroup?.totalCount, 25)
        XCTAssertEqual(repoAGroup?.sessions.count, 10)
        XCTAssertEqual(repoAGroup?.hiddenCount, 15)
        XCTAssertEqual(repoBGroup?.totalCount, 15)
        XCTAssertEqual(repoBGroup?.sessions.count, 10)
        XCTAssertEqual(repoBGroup?.hiddenCount, 5)
    }

    private func makeSession(
        idSuffix: String,
        cwd: String,
        activityUnixMs: UInt64,
        title: String
    ) -> SessionSummary {
        SessionSummary(
            id: UUID(uuidString: idSuffix)!,
            conversationId: idSuffix,
            model: "gpt-5.3-codex",
            cwd: cwd,
            createdAtUnixMs: activityUnixMs,
            lastEventAtUnixMs: activityUnixMs,
            title: title
        )
    }
}
