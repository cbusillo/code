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
    func testStorePrependsHistoryPagesAndTracksHasMoreFlag() throws {
        let store = SessionMirrorStore()
        let sessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000119")!

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

        try applyServerEnvelope(
            """
            {
              "type": "session_attached",
              "session_id": "\(sessionId.uuidString)",
              "from_seq": 0,
              "has_more_before": true,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":10,"level":"info","message":"ten"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":11,"level":"info","message":"eleven"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":12,"level":"info","message":"twelve"}
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.itemsBySession[sessionId]?.map(\.seq), [10, 11, 12])
        XCTAssertTrue(store.hasMoreHistoryBefore(sessionId))

        store.recordPendingHistoryPageRequestForTesting(
            requestId: "native_777",
            sessionID: sessionId
        )

        try applyServerEnvelope(
            """
            {
              "type": "session_history_page",
              "request_id": "native_777",
              "session_id": "\(sessionId.uuidString)",
              "before_seq": 10,
              "has_more_before": true,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":7,"level":"info","message":"seven"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":8,"level":"info","message":"eight"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":9,"level":"info","message":"nine"}
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.itemsBySession[sessionId]?.map(\.seq), [7, 8, 9, 10, 11, 12])
        XCTAssertTrue(store.hasMoreHistoryBefore(sessionId))

        store.recordPendingHistoryPageRequestForTesting(
            requestId: "native_778",
            sessionID: sessionId
        )

        try applyServerEnvelope(
            """
            {
              "type": "session_history_page",
              "request_id": "native_778",
              "session_id": "\(sessionId.uuidString)",
              "before_seq": 7,
              "has_more_before": false,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":1,"level":"info","message":"one"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":2,"level":"info","message":"two"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":3,"level":"info","message":"three"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":4,"level":"info","message":"four"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":5,"level":"info","message":"five"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":6,"level":"info","message":"six"}
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.itemsBySession[sessionId]?.map(\.seq), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
        XCTAssertFalse(store.hasMoreHistoryBefore(sessionId))
    }

    @MainActor
    func testStoreIgnoresUnexpectedHistoryPageResponses() throws {
        let store = SessionMirrorStore()
        let sessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000129")!

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

        try applyServerEnvelope(
            """
            {
              "type": "session_attached",
              "session_id": "\(sessionId.uuidString)",
              "from_seq": 0,
              "has_more_before": true,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":10,"level":"info","message":"ten"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":11,"level":"info","message":"eleven"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":12,"level":"info","message":"twelve"}
              ]
            }
            """,
            to: store
        )

        try applyServerEnvelope(
            """
            {
              "type": "session_history_page",
              "request_id": "native_unexpected",
              "session_id": "\(sessionId.uuidString)",
              "before_seq": 10,
              "has_more_before": true,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":7,"level":"info","message":"seven"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":8,"level":"info","message":"eight"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":9,"level":"info","message":"nine"}
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.itemsBySession[sessionId]?.map(\.seq), [10, 11, 12])
        XCTAssertTrue(store.hasMoreHistoryBefore(sessionId))
    }

    @MainActor
    func testStoreClearsInFlightHistoryBookkeepingForMismatchedResponseSession() throws {
        let store = SessionMirrorStore()
        let sessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000129")!
        let staleSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000130")!

        try applyServerEnvelope(
            """
            {
              "type": "hello",
              "client_id": "client-1"
            }
            """,
            to: store
        )

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

        try applyServerEnvelope(
            """
            {
              "type": "session_attached",
              "session_id": "\(sessionId.uuidString)",
              "from_seq": 0,
              "has_more_before": true,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":10,"level":"info","message":"ten"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":11,"level":"info","message":"eleven"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":12,"level":"info","message":"twelve"}
              ]
            }
            """,
            to: store
        )

        store.recordPendingHistoryPageRequestForTesting(
            requestId: "native_inflight",
            sessionID: sessionId
        )
        XCTAssertTrue(store.isLoadingOlderHistory(for: sessionId))

        try applyServerEnvelope(
            """
            {
              "type": "session_history_page",
              "request_id": "native_inflight",
              "session_id": "\(staleSessionId.uuidString)",
              "before_seq": 10,
              "has_more_before": true,
              "items": [
                {"type":"system","session_id":"\(staleSessionId.uuidString)","seq":7,"level":"info","message":"seven"}
              ]
            }
            """,
            to: store
        )

        XCTAssertFalse(store.isLoadingOlderHistory(for: sessionId))
        XCTAssertTrue(store.hasMoreHistoryBefore(sessionId))
        XCTAssertTrue(store.requestOlderHistoryIfNeeded(for: sessionId))
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
    func testStoreTrimsInactiveSessionHistoryToBoundMemoryGrowth() throws {
        let store = SessionMirrorStore()
        let olderSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000811")!
        let selectedSessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000899")!

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
                  "last_event_at_unix_ms": 1,
                  "title": "Older"
                },
                {
                  "id": "\(selectedSessionId.uuidString)",
                  "conversation_id": "\(selectedSessionId.uuidString)",
                  "model": "gpt-5",
                  "cwd": "/tmp/repo",
                  "created_at_unix_ms": 2,
                  "last_event_at_unix_ms": 2,
                  "title": "Selected"
                }
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.selectedSessionID, selectedSessionId)

        let items = (1...2_000)
            .map { seq in
                """
                {"type":"system","session_id":"\(olderSessionId.uuidString)","seq":\(seq),"level":"info","message":"item-\(seq)"}
                """
            }
            .joined(separator: ",")

        try applyServerEnvelope(
            """
            {
              "type": "session_attached",
              "session_id": "\(olderSessionId.uuidString)",
              "from_seq": 0,
              "has_more_before": false,
              "items": [
                \(items)
              ]
            }
            """,
            to: store
        )

        let retained = try XCTUnwrap(store.itemsBySession[olderSessionId])
        XCTAssertEqual(retained.count, 1_500)
        XCTAssertEqual(retained.first?.seq, 501)
        XCTAssertEqual(retained.last?.seq, 2_000)
        XCTAssertTrue(store.hasMoreHistoryBefore(olderSessionId))
        XCTAssertGreaterThan(store.historyPageTelemetry.inactiveRetentionPasses, 0)
        XCTAssertGreaterThan(store.historyPageTelemetry.inactiveRetentionTrimmedItems, 0)
    }

    @MainActor
    func testRuntimeStateTracksHistoryLifecycleAndUnavailableState() throws {
        let store = SessionMirrorStore()
        let sessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000911")!

        try applyServerEnvelope(
            """
            {
              "type": "hello",
              "client_id": "client-1"
            }
            """,
            to: store
        )

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
                  "title": "State Test"
                }
              ]
            }
            """,
            to: store
        )

        try applyServerEnvelope(
            """
            {
              "type": "session_attached",
              "session_id": "\(sessionId.uuidString)",
              "from_seq": 0,
              "has_more_before": true,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":10,"level":"info","message":"ten"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":11,"level":"info","message":"eleven"}
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.runtimeState(for: sessionId), .connected)

        store.recordPendingHistoryPageRequestForTesting(
            requestId: "native_900",
            sessionID: sessionId
        )
        XCTAssertEqual(store.runtimeState(for: sessionId), .historyLoading)

        try applyServerEnvelope(
            """
            {
              "type": "session_history_page",
              "request_id": "native_900",
              "session_id": "\(sessionId.uuidString)",
              "before_seq": 10,
              "has_more_before": false,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":1,"level":"info","message":"one"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":2,"level":"info","message":"two"}
              ]
            }
            """,
            to: store
        )

        XCTAssertEqual(store.runtimeState(for: sessionId), .historyComplete)

        store.recordPendingAttachRequestForTesting(requestId: "native_901", sessionID: sessionId)
        try applyServerEnvelope(
            """
            {
              "type": "error",
              "request_id": "native_901",
              "message": "Failed to resume session \(sessionId.uuidString): internal error"
            }
            """,
            to: store
        )

        XCTAssertEqual(store.runtimeState(for: sessionId), .unavailable)
    }

    @MainActor
    func testLoadBenchmarkFixtureSeedsDeterministicState() throws {
        let store = SessionMirrorStore()
        let sessionId = UUID(uuidString: "00000000-0000-0000-0000-000000000921")!
        let fixturePath = FileManager.default.temporaryDirectory
            .appendingPathComponent("native-benchmark-fixture-\(UUID().uuidString).json")

        let fixtureJSON = """
        {
          "connection_state": "connected",
          "status_line": "Connected as fixture",
          "client_id": "fixture-client",
          "selected_session_id": "\(sessionId.uuidString)",
          "sessions": [
            {
              "summary": {
                "id": "\(sessionId.uuidString)",
                "conversation_id": "\(sessionId.uuidString)",
                "model": "gpt-5",
                "cwd": "/tmp/repo",
                "created_at_unix_ms": 1,
                "last_event_at_unix_ms": 2,
                "title": "Fixture session"
              },
              "has_more_history_before": true,
              "history_page_in_flight": true,
              "items": [
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":1,"level":"info","message":"one"},
                {"type":"system","session_id":"\(sessionId.uuidString)","seq":2,"level":"info","message":"two"}
              ]
            }
          ],
          "history_page_telemetry": {
            "request_count": 3,
            "success_count": 3,
            "slow_page_count": 1,
            "total_latency_ms": 420
          }
        }
        """

        try fixtureJSON.write(to: fixturePath, atomically: true, encoding: .utf8)
        defer {
            try? FileManager.default.removeItem(at: fixturePath)
        }

        store.loadBenchmarkFixture(from: fixturePath.path)

        XCTAssertEqual(store.connectionState, .connected)
        XCTAssertEqual(store.statusLine, "Connected as fixture")
        XCTAssertEqual(store.selectedSessionID, sessionId)
        XCTAssertEqual(store.selectedSession?.title, "Fixture session")
        XCTAssertEqual(store.selectedSessionItems.map(\.seq), [1, 2])
        XCTAssertTrue(store.hasMoreHistoryBefore(sessionId))
        XCTAssertTrue(store.isLoadingOlderHistory(for: sessionId))
        XCTAssertEqual(store.selectedSessionRuntimeState, .historyLoading)
        XCTAssertEqual(store.historyPageTelemetry.requestCount, 3)
        XCTAssertEqual(store.historyPageTelemetry.successCount, 3)
        XCTAssertEqual(store.historyPageTelemetry.slowPageCount, 1)
        XCTAssertEqual(Int(store.historyPageTelemetry.totalLatencyMs), 420)
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
