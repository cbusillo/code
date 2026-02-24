import XCTest
@testable import CodeNativeApp

final class TranscriptReductionTests: XCTestCase {
    func testDedupeAssistantMessagesDropsDeltaWhenFinalMessageExists() {
        let sessionId = UUID(uuidString: "00000000-0000-0000-0000-00000000de01")!
        let items = [
            makeCoreEventItem(
                sessionId: sessionId,
                seq: 1,
                payload: .object([
                    "type": .string("user_message"),
                    "message": .string("run dogfood")
                ])
            ),
            makeCoreEventItem(
                sessionId: sessionId,
                seq: 2,
                payload: .object([
                    "type": .string("agent_message_delta"),
                    "delta": .string("Interim streaming text")
                ])
            ),
            makeCoreEventItem(
                sessionId: sessionId,
                seq: 3,
                payload: .object([
                    "type": .string("agent_message"),
                    "message": .string("Final response text")
                ])
            )
        ]

        let reduced = dedupeAssistantMessagesWithinTurnItems(items)
        XCTAssertEqual(reduced.map(\.seq), [1, 3])
    }

    func testDedupeAssistantMessagesKeepsDeltaWhenFinalMessageMissing() {
        let sessionId = UUID(uuidString: "00000000-0000-0000-0000-00000000de02")!
        let items = [
            makeCoreEventItem(
                sessionId: sessionId,
                seq: 1,
                payload: .object([
                    "type": .string("user_message"),
                    "message": .string("run dogfood")
                ])
            ),
            makeCoreEventItem(
                sessionId: sessionId,
                seq: 2,
                payload: .object([
                    "type": .string("agent_message_delta"),
                    "delta": .string("Streaming in progress")
                ])
            )
        ]

        let reduced = dedupeAssistantMessagesWithinTurnItems(items)
        XCTAssertEqual(reduced.map(\.seq), [1, 2])
    }

    private func makeCoreEventItem(
        sessionId: UUID,
        seq: UInt64,
        payload: JSONValue
    ) -> SessionStreamItem {
        SessionStreamItem(
            type: "core_event",
            sessionId: sessionId,
            seq: seq,
            event: CoreEventPayload(
                id: "event-\(seq)",
                eventSeq: seq,
                kind: "event",
                payload: payload
            ),
            rev: nil,
            text: nil,
            cursor: nil,
            sourceClientId: nil,
            level: nil,
            message: nil
        )
    }
}

