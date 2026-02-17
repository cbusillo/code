import XCTest
@testable import CodeNativeApp

final class SessionStreamReducerTests: XCTestCase {
    func testMergeReplayItemsInitialReplaySortsAndDeduplicates() {
        let incoming = [
            makeSystemItem(seq: 5),
            makeSystemItem(seq: 3),
            makeSystemItem(seq: 3),
            makeSystemItem(seq: 4),
        ]

        let merged = SessionStreamReducer.mergeReplayItems(
            existing: [],
            incoming: incoming,
            fromSeq: 0,
            maxItems: 10
        )

        XCTAssertEqual(merged.map(\.seq), [3, 4, 5])
    }

    func testMergeReplayItemsIncrementalIgnoresOlderReplayRows() {
        let existing = [
            makeSystemItem(seq: 101),
            makeSystemItem(seq: 102),
            makeSystemItem(seq: 103),
        ]
        let incoming = [
            makeSystemItem(seq: 1),
            makeSystemItem(seq: 2),
            makeSystemItem(seq: 101),
            makeSystemItem(seq: 102),
            makeSystemItem(seq: 103),
            makeSystemItem(seq: 104),
            makeSystemItem(seq: 105),
        ]

        let merged = SessionStreamReducer.mergeReplayItems(
            existing: existing,
            incoming: incoming,
            fromSeq: 103,
            maxItems: 10
        )

        XCTAssertEqual(merged.map(\.seq), [101, 102, 103, 104, 105])
    }

    func testAppendLiveItemNormalizesAndRejectsStaleRows() {
        let items = [
            makeSystemItem(seq: 5),
            makeSystemItem(seq: 3),
            makeSystemItem(seq: 4),
        ]

        let unchanged = SessionStreamReducer.appendLiveItem(
            items: items,
            newItem: makeSystemItem(seq: 4),
            maxItems: 10
        )

        XCTAssertEqual(unchanged.map(\.seq), [3, 4, 5])
    }

    func testAppendLiveItemAppendsNewRowsAfterNormalization() {
        let items = [
            makeSystemItem(seq: 5),
            makeSystemItem(seq: 3),
            makeSystemItem(seq: 4),
        ]

        let appended = SessionStreamReducer.appendLiveItem(
            items: items,
            newItem: makeSystemItem(seq: 6),
            maxItems: 10
        )

        XCTAssertEqual(appended.map(\.seq), [3, 4, 5, 6])
    }

    func testMergeOlderHistoryPagePrependsUniqueRows() {
        let existing = [
            makeSystemItem(seq: 8),
            makeSystemItem(seq: 9),
            makeSystemItem(seq: 10),
        ]

        let incoming = [
            makeSystemItem(seq: 5),
            makeSystemItem(seq: 6),
            makeSystemItem(seq: 7),
            makeSystemItem(seq: 8),
            makeSystemItem(seq: 8),
        ]

        let merged = SessionStreamReducer.mergeOlderHistoryPage(
            existing: existing,
            incoming: incoming,
            beforeSeq: 8,
            maxItems: 20
        )

        XCTAssertEqual(merged.map(\.seq), [5, 6, 7, 8, 9, 10])
    }

    func testShouldAcceptSessionAttachedRejectsStaleExpectedSession() {
        let selected = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!
        let expected = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!
        let attached = UUID(uuidString: "00000000-0000-0000-0000-000000000002")!

        XCTAssertFalse(
            SessionStreamReducer.shouldAcceptSessionAttached(
                selectedSessionID: selected,
                expectedSessionID: expected,
                attachedSessionID: attached
            )
        )
    }

    func testShouldAcceptSessionAttachedRequiresSelectedSessionMatch() {
        let selected = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!
        let attached = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!

        XCTAssertTrue(
            SessionStreamReducer.shouldAcceptSessionAttached(
                selectedSessionID: selected,
                expectedSessionID: nil,
                attachedSessionID: attached
            )
        )

        XCTAssertFalse(
            SessionStreamReducer.shouldAcceptSessionAttached(
                selectedSessionID: nil,
                expectedSessionID: nil,
                attachedSessionID: attached
            )
        )
    }

    private func makeSystemItem(seq: UInt64) -> SessionStreamItem {
        SessionStreamItem(
            type: "system",
            sessionId: UUID(uuidString: "00000000-0000-0000-0000-000000000001")!,
            seq: seq,
            event: nil,
            rev: nil,
            text: nil,
            cursor: nil,
            sourceClientId: nil,
            level: "info",
            message: "m\(seq)"
        )
    }
}
