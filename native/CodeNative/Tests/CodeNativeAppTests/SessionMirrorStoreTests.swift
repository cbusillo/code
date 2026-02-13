import XCTest
@testable import CodeNativeApp

final class SessionMirrorStoreTests: XCTestCase {
    @MainActor
    func testStoreDropsStaleLiveItemsAndMergesIncrementalReplayWithoutDuplicates() throws {
        let store = SessionMirrorStore()
        let sessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000111")!

        try applyServerEnvelope(
            """
            {
              "type": "session_list",
              "sessions": [
                {
                  "id": "\(sessionId.uuidString)",
                  "conversation_id": "\(sessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 1,
                  "last_event_at_unix_ms": 1,
                  "title": "Session"
                }
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, sessionId)

        try applyServerEnvelope(
            """
            {
              "type": "session_attached",
              "session_id": "\(sessionId.uuidString)",
              "from_seq": 0,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":5,"level":"info","message":"five"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":3,"level":"info","message":"three"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":3,"level":"info","message":"three-duplicate"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":4,"level":"info","message":"four"}
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.itemsBySession[sessionId]?.map(\.seq), [3, 4, 5])

        // Stale live seq should be ignored.
        try applyServerEnvelope(
            """
            {
              "type": "session_stream",
              "item": {
                "type": "system",
                "session_id": "\(sessionId.uuidString)",
                "seq": 4,
                "level": "info",
                "message": "stale"
              }
            }
            """,
            to: store
        )

        XCTAssertEqual(store.itemsBySession[sessionId]?.map(\.seq), [3, 4, 5])

        // New live seq should append.
        try applyServerEnvelope(
            """
            {
              "type": "session_stream",
              "item": {
                "type": "system",
                "session_id": "\(sessionId.uuidString)",
                "seq": 6,
                "level": "info",
                "message": "six"
              }
            }
            """,
            to: store
        )

        XCTAssertEqual(store.itemsBySession[sessionId]?.map(\.seq), [3, 4, 5, 6])

        // Incremental replay should only append seqs above the current high water.
        try applyServerEnvelope(
            """
            {
              "type": "session_attached",
              "session_id": "\(sessionId.uuidString)",
              "from_seq": 5,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":2,"level":"info","message":"two"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":5,"level":"info","message":"five"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":6,"level":"info","message":"six"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":6,"level":"info","message":"six-dup"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":7,"level":"info","message":"seven"}
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.itemsBySession[sessionId]?.map(\.seq), [3, 4, 5, 6, 7])
    }

    @MainActor
    func testStoreRejectsStaleSessionAttachedForAttachmentState() throws {
        let store = SessionMirrorStore()
        let selectedSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000211")!
        let staleSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000299")!

        try applyServerEnvelope(
            """
            {
              "type": "session_list",
              "sessions": [
                {
                  "id": "\(selectedSessionId.uuidString)",
                  "conversation_id": "\(selectedSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 1,
                  "last_event_at_unix_ms": 1,
                  "title": "Selected"
                }
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, selectedSessionId)

        try applyServerEnvelope(
            """
            {
              "type": "session_attached",
              "session_id": "\(staleSessionId.uuidString)",
              "from_seq": 0,
              "items": [
                {"type":"system","session_id":"\(staleSessionId.uuidString)","seq":1,"level":"info","message":"stale"}
              ]
            }
            """,
            to: store
        )

        XCTAssertFalse(store.statusLine.contains(staleSessionId.uuidString.prefix(8)))
    }

    @MainActor
    private func applyServerEnvelope(
        _ rawJSON: String,
        to store: SessionMirrorStore
    ) throws {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let envelope = try decoder.decode(ServerEnvelope.self, from: Data(rawJSON.utf8))
        store.applyEnvelopeForTesting(envelope)
    }
}
