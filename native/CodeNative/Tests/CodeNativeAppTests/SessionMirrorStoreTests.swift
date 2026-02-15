import XCTest
@testable import CodeNativeApp

final class SessionMirrorStoreTests: XCTestCase {
    @MainActor
    func testConnectRejectsNonLoopbackEndpoint() async {
        let store = SessionMirrorStore()
        store.endpoint = "ws://example.com:4317/ws"

        await store.connect()

        XCTAssertEqual(store.connectionState, .disconnected)
        XCTAssertEqual(store.statusLine, "Loopback endpoints only")
        XCTAssertEqual(
            store.lastError,
            "Endpoint must use ws://localhost, ws://127.0.0.1, or ws://[::1]."
        )
    }

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
    func testStoreDecodeFailureIncrementsTransportFailureTelemetry() {
        let store = SessionMirrorStore()

        store.ingestRawPayloadForTesting("{ this is not valid json }")

        XCTAssertEqual(store.transportFailureCount, 1)
        XCTAssertNotNil(store.lastTransportFailureAt)
        XCTAssertTrue((store.lastError ?? "").hasPrefix("Failed to decode server message:"))
    }

    @MainActor
    func testAttachErrorMarksSessionUnavailableAndClearsAfterSuccessfulAttach() throws {
        let store = SessionMirrorStore()
        let sessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000311")!
        let requestId = "native_311"
        let errorMessage = "Failed to resume session \(sessionId.uuidString): internal error; agent loop died unexpectedly"

        store.recordPendingAttachRequestForTesting(requestId: requestId, sessionID: sessionId)

        try applyServerEnvelope(
            """
            {
              "type": "error",
              "request_id": "\(requestId)",
              "message": "\(errorMessage)"
            }
            """,
            to: store
        )

        XCTAssertEqual(store.unavailableSessionError(for: sessionId), errorMessage)

        try applyServerEnvelope(
            """
            {
              "type": "session_attached",
              "session_id": "\(sessionId.uuidString)",
              "from_seq": 0,
              "items": []
            }
            """,
            to: store
        )

        XCTAssertNil(store.unavailableSessionError(for: sessionId))
    }

    @MainActor
    func testNonAttachErrorDoesNotMarkSessionUnavailable() throws {
        let store = SessionMirrorStore()

        try applyServerEnvelope(
            """
            {
              "type": "error",
              "request_id": "native_901",
              "message": "Failed to submit turn: missing session"
            }
            """,
            to: store
        )

        XCTAssertTrue(store.unavailableSessionIDs.isEmpty)
    }

    @MainActor
    func testAttachErrorOnSelectedSessionAutoSelectsNextAvailableSession() throws {
        let store = SessionMirrorStore()
        let staleSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000411")!
        let healthySessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000499")!
        let requestId = "native_411"

        try applyServerEnvelope(
            """
            {
              "type": "session_list",
              "sessions": [
                {
                  "id": "\(healthySessionId.uuidString)",
                  "conversation_id": "\(healthySessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 1,
                  "last_event_at_unix_ms": 1,
                  "title": "Healthy"
                },
                {
                  "id": "\(staleSessionId.uuidString)",
                  "conversation_id": "\(staleSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 2,
                  "last_event_at_unix_ms": 2,
                  "title": "Stale"
                }
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, staleSessionId)

        store.recordPendingAttachRequestForTesting(requestId: requestId, sessionID: staleSessionId)
        try applyServerEnvelope(
            """
            {
              "type": "error",
              "request_id": "\(requestId)",
              "message": "Failed to resume session \(staleSessionId.uuidString): internal error"
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, healthySessionId)
        XCTAssertNotNil(store.unavailableSessionError(for: staleSessionId))
    }

    @MainActor
    func testSessionActivityClearsUnavailableState() throws {
        let store = SessionMirrorStore()
        let staleSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000421")!
        let requestId = "native_421"

        try applyServerEnvelope(
            """
            {
              "type": "session_list",
              "sessions": [
                {
                  "id": "\(staleSessionId.uuidString)",
                  "conversation_id": "\(staleSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 1,
                  "last_event_at_unix_ms": 2,
                  "title": "Demo"
                }
              ]
            }
            """,
            to: store
        )

        store.recordPendingAttachRequestForTesting(requestId: requestId, sessionID: staleSessionId)
        try applyServerEnvelope(
            """
            {
              "type": "error",
              "request_id": "\(requestId)",
              "message": "Failed to resume session \(staleSessionId.uuidString): internal error"
            }
            """,
            to: store
        )

        XCTAssertNotNil(store.unavailableSessionError(for: staleSessionId))

        try applyServerEnvelope(
            """
            {
              "type": "session_list",
              "sessions": [
                {
                  "id": "\(staleSessionId.uuidString)",
                  "conversation_id": "\(staleSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 1,
                  "last_event_at_unix_ms": 9,
                  "title": "Demo"
                }
              ]
            }
            """,
            to: store
        )

        XCTAssertNil(store.unavailableSessionError(for: staleSessionId))
    }

    @MainActor
    func testSessionListReplacesUnavailableSelectedSession() throws {
        let store = SessionMirrorStore()
        let staleSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000511")!
        let healthySessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000599")!
        let requestId = "native_511"

        try applyServerEnvelope(
            """
            {
              "type": "session_list",
              "sessions": [
                {
                  "id": "\(healthySessionId.uuidString)",
                  "conversation_id": "\(healthySessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 1,
                  "last_event_at_unix_ms": 1,
                  "title": "Healthy"
                },
                {
                  "id": "\(staleSessionId.uuidString)",
                  "conversation_id": "\(staleSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 2,
                  "last_event_at_unix_ms": 2,
                  "title": "Stale"
                }
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, staleSessionId)

        store.recordPendingAttachRequestForTesting(requestId: requestId, sessionID: staleSessionId)
        try applyServerEnvelope(
            """
            {
              "type": "error",
              "request_id": "\(requestId)",
              "message": "Failed to resume session \(staleSessionId.uuidString): internal error"
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, healthySessionId)

        // A subsequent session_list should keep the healthy thread selected and
        // avoid snapping back to the known-unavailable one.
        try applyServerEnvelope(
            """
            {
              "type": "session_list",
              "sessions": [
                {
                  "id": "\(healthySessionId.uuidString)",
                  "conversation_id": "\(healthySessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 1,
                  "last_event_at_unix_ms": 3,
                  "title": "Healthy"
                },
                {
                  "id": "\(staleSessionId.uuidString)",
                  "conversation_id": "\(staleSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 2,
                  "last_event_at_unix_ms": 4,
                  "title": "Stale"
                }
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, healthySessionId)
    }

    @MainActor
    func testAutoSelectPrefersUsefulTitledSessionOverUntitledNewestSession() throws {
        let store = SessionMirrorStore()
        let titledSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000611")!
        let untitledNewestSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000699")!

        try applyServerEnvelope(
            """
            {
              "type": "session_list",
              "sessions": [
                {
                  "id": "\(titledSessionId.uuidString)",
                  "conversation_id": "\(titledSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 1,
                  "last_event_at_unix_ms": 10,
                  "title": "Fix iOS layout spacing"
                },
                {
                  "id": "\(untitledNewestSessionId.uuidString)",
                  "conversation_id": "\(untitledNewestSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 2,
                  "last_event_at_unix_ms": 20,
                  "title": null
                }
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, titledSessionId)
    }

    @MainActor
    func testAutoSelectFallsBackToNewestWhenNoUsefulTitleExists() throws {
        let store = SessionMirrorStore()
        let olderSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000711")!
        let newestSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000799")!

        try applyServerEnvelope(
            """
            {
              "type": "session_list",
              "sessions": [
                {
                  "id": "\(olderSessionId.uuidString)",
                  "conversation_id": "\(olderSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 1,
                  "last_event_at_unix_ms": 10,
                  "title": "go ahead"
                },
                {
                  "id": "\(newestSessionId.uuidString)",
                  "conversation_id": "\(newestSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 2,
                  "last_event_at_unix_ms": 20,
                  "title": null
                }
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, newestSessionId)
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
