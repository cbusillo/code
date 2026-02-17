import Foundation
import OSLog

@MainActor
final class SessionMirrorStore: ObservableObject {
    private static let logger = Logger(subsystem: "com.every.code.native", category: "session-mirror")
    // Large catalogs can contain hundreds of sessions; keep periodic refreshes
    // sparse so list decoding doesn't interrupt typing responsiveness.
    private static let catalogRefreshIntervalNanoseconds: UInt64 = 15_000_000_000
    private static let sessionListRequestTimeoutSeconds: TimeInterval = 20
    private static let historyPageRequestThrottleSeconds: TimeInterval = 0.35
    private static let slowHistoryPageThresholdMs: Double = 350

    struct HistoryPageTelemetry {
        var requestCount: UInt64 = 0
        var successCount: UInt64 = 0
        // Counts pages likely to hitch rendering; this is a proxy, not actual dropped-frame telemetry.
        var slowPageCount: UInt64 = 0
        var totalLatencyMs: Double = 0

        var averageLatencyMs: Double {
            guard successCount > 0 else {
                return 0
            }
            return totalLatencyMs / Double(successCount)
        }
    }

    enum ConnectionState: String {
        case disconnected
        case connecting
        case connected
    }

    @Published var endpoint: String = "ws://127.0.0.1:4317/ws"
    @Published private(set) var connectionState: ConnectionState = .disconnected
    @Published private(set) var statusLine: String = "Disconnected"
    @Published private(set) var sessions: [SessionSummary] = []
    @Published private(set) var itemsBySession: [UUID: [SessionStreamItem]] = [:]
    @Published private(set) var unavailableSessionErrors: [UUID: String] = [:]
    @Published private(set) var historyPageLoadInFlightSessionIDs: Set<UUID> = []
    @Published private(set) var historyPageTelemetry = HistoryPageTelemetry()
    @Published var composerText: String = ""
    @Published var selectedSessionID: UUID? {
        didSet {
            guard oldValue != selectedSessionID else {
                return
            }
            attachmentGeneration = attachmentGeneration.saturatingIncrement()
            let generation = attachmentGeneration
            expectedAttachedSessionID = selectedSessionID
            Task {
                await self.switchAttachment(
                    from: oldValue,
                    to: self.selectedSessionID,
                    generation: generation
                )
            }
        }
    }

    @Published private(set) var lastError: String?
    @Published private(set) var transportFailureCount: UInt64 = 0
    @Published private(set) var lastTransportFailureAt: Date?

    private var webSocket: URLSessionWebSocketTask?
    private var receiveTask: Task<Void, Never>?
    private var reconnectTask: Task<Void, Never>?
    private var catalogRefreshTask: Task<Void, Never>?
    private var attachedSessionID: UUID?
    private var expectedAttachedSessionID: UUID?
    private var clientId: String?
    private var userInitiatedDisconnect: Bool = false
    private var requestCounter: UInt64 = 0
    private var attachmentGeneration: UInt64 = 0
    private var pendingAttachRequestSessionIDs: [String: UUID] = [:]
    private var pendingHistoryPageRequestSessionIDs: [String: UUID] = [:]
    private var historyPageRequestStartedAtByRequestID: [String: Date] = [:]
    private var lastHistoryPageRequestAtBySessionID: [UUID: Date] = [:]
    private var hasMoreHistoryBeforeBySessionID: [UUID: Bool] = [:]
    private var unavailableSessionActivitySnapshot: [UUID: UInt64] = [:]
    private var sessionListRequestID: String?
    private var sessionListRequestStartedAt: Date?

    private lazy var decoder: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }()

    private lazy var encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        return encoder
    }()

    var selectedSession: SessionSummary? {
        guard let selectedSessionID else {
            return nil
        }
        return sessions.first(where: { $0.id == selectedSessionID })
    }

    var selectedSessionItems: [SessionStreamItem] {
        guard let selectedSessionID else {
            return []
        }
        return itemsBySession[selectedSessionID] ?? []
    }

    var unavailableSessionIDs: Set<UUID> {
        Set(unavailableSessionErrors.keys)
    }

    func isSessionUnavailable(_ sessionID: UUID) -> Bool {
        unavailableSessionErrors[sessionID] != nil
    }

    func unavailableSessionError(for sessionID: UUID) -> String? {
        unavailableSessionErrors[sessionID]
    }

    func hasMoreHistoryBefore(_ sessionID: UUID) -> Bool {
        if let hasMore = hasMoreHistoryBeforeBySessionID[sessionID] {
            return hasMore
        }

        if let oldestSeq = itemsBySession[sessionID]?.first?.seq {
            return oldestSeq > 1
        }

        return false
    }

    func isLoadingOlderHistory(for sessionID: UUID) -> Bool {
        historyPageLoadInFlightSessionIDs.contains(sessionID)
    }

    func connect() async {
        guard connectionState == .disconnected else {
            return
        }

        guard let url = URL(string: endpoint) else {
            statusLine = "Invalid URL"
            lastError = "Endpoint must be a valid WebSocket URL."
            return
        }

        guard endpointIsAllowed(url) else {
            statusLine = "Loopback endpoints only"
            lastError = "Endpoint must use ws://localhost, ws://127.0.0.1, or ws://[::1]."
            return
        }

        connectionState = .connecting
        statusLine = "Connecting..."
        lastError = nil
        userInitiatedDisconnect = false
        reconnectTask?.cancel()
        reconnectTask = nil
        catalogRefreshTask?.cancel()
        catalogRefreshTask = nil

        let task = URLSession.shared.webSocketTask(with: url)
        task.maximumMessageSize = 8_000_000
        task.resume()

        webSocket = task

        receiveTask = Task {
            await self.receiveLoop()
        }
    }

    func disconnect() {
        userInitiatedDisconnect = true
        cleanupConnection(status: "Disconnected", error: nil)
    }

    func refreshSessions() async {
        if let sessionListRequestID,
           let startedAt = sessionListRequestStartedAt,
           Date().timeIntervalSince(startedAt) < Self.sessionListRequestTimeoutSeconds {
            Self.logger.debug("Skipping list_sessions; request \(sessionListRequestID, privacy: .public) still pending")
            return
        }

        let requestId = nextRequestID()
        sessionListRequestID = requestId
        sessionListRequestStartedAt = Date()
        await send(OutboundMessage.listSessions(requestId: requestId))
    }

    func createSession(cwd: String?) async {
        let normalized = cwd?.trimmingCharacters(in: .whitespacesAndNewlines)
        let value = normalized?.isEmpty == true ? nil : normalized
        await send(CreateSessionMessage(requestId: nextRequestID(), cwd: value))
    }

    func submitComposer(text explicitText: String? = nil) async {
        guard let selectedSessionID else {
            return
        }

        let text = (explicitText ?? composerText).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            return
        }

        let cursor = text.count

        await send(
            ComposerUpdateMessage(
                requestId: nextRequestID(),
                sessionId: selectedSessionID,
                text: text,
                cursor: cursor
            )
        )
        await send(SubmitTurnMessage(requestId: nextRequestID(), sessionId: selectedSessionID))
        composerText = ""
    }

    func interruptTurn() async {
        guard let selectedSessionID else {
            return
        }
        await send(InterruptTurnMessage(requestId: nextRequestID(), sessionId: selectedSessionID))
    }

    func submitApproval(
        sessionId: UUID,
        callId: String,
        type: ApprovalType,
        decision: ApprovalDecisionChoice
    ) async {
        let wireDecision = decision.wireValue
        switch type {
        case .exec:
            await send(
                ExecApprovalMessage(
                    requestId: nextRequestID(),
                    sessionId: sessionId,
                    callId: callId,
                    decision: wireDecision
                )
            )
        case .patch:
            await send(
                PatchApprovalMessage(
                    requestId: nextRequestID(),
                    sessionId: sessionId,
                    callId: callId,
                    decision: wireDecision
                )
            )
        }
    }

    private func cleanupConnection(status: String, error: String?) {
        receiveTask?.cancel()
        receiveTask = nil
        reconnectTask?.cancel()
        reconnectTask = nil
        catalogRefreshTask?.cancel()
        catalogRefreshTask = nil

        webSocket?.cancel(with: .goingAway, reason: nil)
        webSocket = nil

        attachedSessionID = nil
        expectedAttachedSessionID = nil
        clientId = nil
        attachmentGeneration = attachmentGeneration.saturatingIncrement()
        pendingAttachRequestSessionIDs.removeAll()
        pendingHistoryPageRequestSessionIDs.removeAll()
        historyPageRequestStartedAtByRequestID.removeAll()
        lastHistoryPageRequestAtBySessionID.removeAll()
        hasMoreHistoryBeforeBySessionID.removeAll()
        historyPageLoadInFlightSessionIDs.removeAll()
        sessionListRequestID = nil
        sessionListRequestStartedAt = nil
        if error != nil {
            lastError = error
            recordTransportFailure(context: status, details: error)
        }
        connectionState = .disconnected
        statusLine = statusLineForDisconnect(status: status, error: error)

        if error != nil,
           !userInitiatedDisconnect {
            scheduleReconnect()
        }
    }

    private func startCatalogRefreshLoopIfNeeded() {
        guard catalogRefreshTask == nil else {
            return
        }

        catalogRefreshTask = Task {
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: Self.catalogRefreshIntervalNanoseconds)
                guard !Task.isCancelled else {
                    return
                }

                guard self.connectionState == .connected else {
                    continue
                }

                await self.refreshSessions()
            }
        }
    }

    private func scheduleReconnect() {
        guard reconnectTask == nil else {
            return
        }

        reconnectTask = Task {
            try? await Task.sleep(nanoseconds: 900_000_000)
            await MainActor.run {
                self.reconnectTask = nil
            }

            guard !Task.isCancelled else {
                return
            }

            await MainActor.run {
                guard !self.userInitiatedDisconnect,
                      self.connectionState == .disconnected
                else {
                    return
                }

                Task {
                    await self.connect()
                }
            }
        }
    }

    private func receiveLoop() async {
        while !Task.isCancelled {
            guard let webSocket else {
                break
            }

            do {
                let message = try await webSocket.receive()

                switch message {
                case .string(let text):
                    handleIncomingText(text)
                case .data(let data):
                    handleIncomingData(data)
                @unknown default:
                    continue
                }
            } catch {
                if Task.isCancelled {
                    return
                }

                cleanupConnection(
                    status: "Disconnected unexpectedly",
                    error: error.localizedDescription
                )
                break
            }
        }
    }

    private func handleIncomingText(_ text: String) {
        guard let data = text.data(using: .utf8) else {
            return
        }
        handleIncomingData(data)
    }

    private func handleIncomingData(_ data: Data) {
        do {
            let envelope = try decoder.decode(ServerEnvelope.self, from: data)
            apply(envelope)
        } catch {
            let description = decodeErrorDescription(error)
            lastError = "Failed to decode server message: \(description)"
            recordTransportFailure(context: "Decode failure", details: description)
        }
    }

    private func decodeErrorDescription(_ error: Error) -> String {
        guard let decodingError = error as? DecodingError else {
            return error.localizedDescription
        }

        switch decodingError {
        case .keyNotFound(let key, let context):
            let path = (context.codingPath + [key]).map(\.stringValue).joined(separator: ".")
            return "missing key '\(path)'"
        case .typeMismatch(_, let context), .valueNotFound(_, let context), .dataCorrupted(let context):
            let path = context.codingPath.map(\.stringValue).joined(separator: ".")
            if path.isEmpty {
                return context.debugDescription
            }
            return "\(context.debugDescription) at '\(path)'"
        @unknown default:
            return error.localizedDescription
        }
    }

    private func apply(_ envelope: ServerEnvelope) {
        switch envelope {
        case .hello(let message):
            clientId = message.clientId
            connectionState = .connected
            statusLine = "Connected as \(message.clientId.prefix(8))"
            startCatalogRefreshLoopIfNeeded()
            Task { @MainActor in
                await self.refreshSessions()
            }

        case .sessionList(let message):
            sessionListRequestID = nil
            sessionListRequestStartedAt = nil
            sessions = message.sessions.sorted(by: { sessionActivityUnixMs($0) < sessionActivityUnixMs($1) })
            pruneUnavailableSessionsToKnownIDs()
            clearRecoveredUnavailableSessions()
            pruneHistoryPaginationStateToKnownSessionIDs()

            if let selectedSessionID,
               sessions.contains(where: { $0.id == selectedSessionID }),
               !isSessionUnavailable(selectedSessionID) {
                ensureAttachmentForSelectedSession(selectedSessionID)
                return
            }

            selectedSessionID = preferredAutoSelectedSession(in: sessions)?.id

        case .sessionCreated(let message):
            upsertSession(message.session)
            selectedSessionID = message.session.id

        case .sessionAttached(let message):
            let existing = itemsBySession[message.sessionId] ?? []
            let merged = SessionStreamReducer.mergeReplayItems(
                existing: existing,
                incoming: message.items,
                fromSeq: message.fromSeq
            )
            itemsBySession[message.sessionId] = merged
            clearPendingAttachRequests(for: message.sessionId)
            historyPageLoadInFlightSessionIDs.remove(message.sessionId)
            if message.fromSeq == 0 {
                hasMoreHistoryBeforeBySessionID[message.sessionId] = message.hasMoreBefore ?? false
            } else if message.hasMoreBefore == true {
                hasMoreHistoryBeforeBySessionID[message.sessionId] = true
            }
            clearSessionUnavailable(message.sessionId)

            if !SessionStreamReducer.shouldAcceptSessionAttached(
                selectedSessionID: selectedSessionID,
                expectedSessionID: expectedAttachedSessionID,
                attachedSessionID: message.sessionId
            ) {
                return
            }

            attachedSessionID = message.sessionId
            expectedAttachedSessionID = nil
            statusLine = "Attached to \(message.sessionId.uuidString.prefix(8))"

        case .sessionHistoryPage(let message):
            if let requestId = message.requestId {
                pendingHistoryPageRequestSessionIDs.removeValue(forKey: requestId)
                if let startedAt = historyPageRequestStartedAtByRequestID.removeValue(forKey: requestId) {
                    let latencyMs = Date().timeIntervalSince(startedAt) * 1_000
                    historyPageTelemetry.successCount += 1
                    historyPageTelemetry.totalLatencyMs += latencyMs
                    if latencyMs > Self.slowHistoryPageThresholdMs {
                        historyPageTelemetry.slowPageCount += 1
                    }
                }
            }
            historyPageLoadInFlightSessionIDs.remove(message.sessionId)

            let existing = itemsBySession[message.sessionId] ?? []
            let merged = SessionStreamReducer.mergeOlderHistoryPage(
                existing: existing,
                incoming: message.items,
                beforeSeq: message.beforeSeq
            )
            itemsBySession[message.sessionId] = merged
            hasMoreHistoryBeforeBySessionID[message.sessionId] = message.hasMoreBefore

        case .sessionDetached(let message):
            if attachedSessionID == message.sessionId {
                attachedSessionID = nil
            }
            statusLine = "Detached from \(message.sessionId.uuidString.prefix(8))"

        case .sessionStream(let message):
            let sessionId = message.item.sessionId
            itemsBySession[sessionId] = SessionStreamReducer.appendLiveItem(
                items: itemsBySession[sessionId] ?? [],
                newItem: message.item
            )

            bumpSessionActivity(sessionId: sessionId)

            if sessionId == selectedSessionID,
               message.item.type == "composer" {
                let isFromCurrentClient = message.item.sourceClientId == clientId
                if !isFromCurrentClient {
                    composerText = message.item.text ?? ""
                }
            }

        case .error(let message):
            if message.requestId == sessionListRequestID {
                sessionListRequestID = nil
                sessionListRequestStartedAt = nil
            }
            lastError = message.message
            if let requestId = message.requestId,
               let historySessionID = pendingHistoryPageRequestSessionIDs.removeValue(forKey: requestId) {
                historyPageRequestStartedAtByRequestID.removeValue(forKey: requestId)
                historyPageLoadInFlightSessionIDs.remove(historySessionID)
                statusLine = "History load failed"
            } else if let requestId = message.requestId,
               let failedSessionID = pendingAttachRequestSessionIDs.removeValue(forKey: requestId) {
                markSessionUnavailable(failedSessionID, message: message.message)
                if selectedSessionID == failedSessionID {
                    statusLine = "Thread unavailable"
                    selectedSessionID = preferredAutoSelectedSession(
                        in: sessions,
                        excluding: [failedSessionID]
                    )?.id
                }
            } else {
                statusLine = "Server error"
            }

        case .ack:
            break

        case .unknown(let type):
            statusLine = "Unhandled message: \(type)"
        }
    }

    private func upsertSession(_ session: SessionSummary) {
        if let index = sessions.firstIndex(where: { $0.id == session.id }) {
            sessions[index] = session
        } else {
            sessions.append(session)
        }
        sessions.sort(by: { sessionActivityUnixMs($0) < sessionActivityUnixMs($1) })
    }

    private func ensureAttachmentForSelectedSession(_ sessionID: UUID) {
        if attachedSessionID == sessionID {
            return
        }

        if expectedAttachedSessionID == sessionID {
            return
        }

        attachmentGeneration = attachmentGeneration.saturatingIncrement()
        let generation = attachmentGeneration
        let previousAttachedSessionID = attachedSessionID
        expectedAttachedSessionID = sessionID

        Task {
            await self.switchAttachment(
                from: previousAttachedSessionID,
                to: sessionID,
                generation: generation
            )
        }
    }

    private func switchAttachment(
        from oldSessionID: UUID?,
        to newSessionID: UUID?,
        generation: UInt64
    ) async {
        guard generation == attachmentGeneration else {
            return
        }

        if let oldSessionID {
            await send(OutboundMessage.detachSession(requestId: nextRequestID(), sessionId: oldSessionID))
            attachedSessionID = nil
            historyPageLoadInFlightSessionIDs.remove(oldSessionID)

            guard generation == attachmentGeneration else {
                return
            }
        }

        guard let newSessionID else {
            return
        }

        let fromSeq = itemsBySession[newSessionID]?.last?.seq ?? 0
        let requestId = nextRequestID()

        guard generation == attachmentGeneration else {
            return
        }

        pendingAttachRequestSessionIDs[requestId] = newSessionID

        await send(
            OutboundMessage.attachSession(
                requestId: requestId,
                sessionId: newSessionID,
                fromSeq: fromSeq
            )
        )
    }

    func requestOlderHistoryIfNeeded(for sessionID: UUID) {
        guard connectionState == .connected else {
            return
        }

        guard let oldestSeq = itemsBySession[sessionID]?.first?.seq else {
            return
        }

        if oldestSeq <= 1 {
            hasMoreHistoryBeforeBySessionID[sessionID] = false
            return
        }

        if historyPageLoadInFlightSessionIDs.contains(sessionID) {
            return
        }

        if let lastRequestAt = lastHistoryPageRequestAtBySessionID[sessionID],
           Date().timeIntervalSince(lastRequestAt) < Self.historyPageRequestThrottleSeconds {
            return
        }

        if hasMoreHistoryBeforeBySessionID[sessionID] == false {
            return
        }

        let requestId = nextRequestID()
        historyPageLoadInFlightSessionIDs.insert(sessionID)
        lastHistoryPageRequestAtBySessionID[sessionID] = Date()
        historyPageRequestStartedAtByRequestID[requestId] = Date()
        historyPageTelemetry.requestCount += 1
        pendingHistoryPageRequestSessionIDs[requestId] = sessionID

        Task {
            await self.send(
                OutboundMessage.loadHistoryBefore(
                    requestId: requestId,
                    sessionId: sessionID,
                    beforeSeq: oldestSeq,
                    limit: 600
                )
            )
        }
    }

    private func send(_ message: OutboundMessage) async {
        await sendEncodable(message)
    }

    private func send(_ message: CreateSessionMessage) async {
        await sendEncodable(message)
    }

    private func send(_ message: ComposerUpdateMessage) async {
        await sendEncodable(message)
    }

    private func send(_ message: SubmitTurnMessage) async {
        await sendEncodable(message)
    }

    private func send(_ message: InterruptTurnMessage) async {
        await sendEncodable(message)
    }

    private func send(_ message: ExecApprovalMessage) async {
        await sendEncodable(message)
    }

    private func send(_ message: PatchApprovalMessage) async {
        await sendEncodable(message)
    }

    private func sendEncodable<T: Encodable>(_ message: T) async {
        guard let webSocket else {
            return
        }

        do {
            let data = try encoder.encode(message)
            guard let text = String(data: data, encoding: .utf8) else {
                return
            }
            try await webSocket.send(.string(text))
        } catch {
            let description = error.localizedDescription
            lastError = "Failed to send message: \(description)"
            recordTransportFailure(context: "Send failure", details: description)
        }
    }

    private func endpointIsAllowed(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased(), scheme == "ws" || scheme == "wss" else {
            return false
        }

        guard let host = url.host?.lowercased() else {
            return false
        }

        return host == "localhost" || host == "127.0.0.1" || host == "::1"
    }

    private func recordTransportFailure(context: String, details: String?) {
        transportFailureCount = transportFailureCount.saturatingIncrement()
        lastTransportFailureAt = Date()

        if let details,
           !details.isEmpty {
            Self.logger.error("\(context, privacy: .public): \(details, privacy: .public)")
            return
        }

        Self.logger.error("\(context, privacy: .public)")
    }

    private func nextRequestID() -> String {
        requestCounter = requestCounter.saturatingIncrement()
        return "native_\(requestCounter)"
    }

    private func statusLineForDisconnect(status: String, error: String?) -> String {
        guard let error else {
            return status
        }

        let normalized = error.lowercased()
        if normalized.contains("could not connect") || normalized.contains("not connected") {
            return "Backend offline; retrying..."
        }
        if normalized.contains("timed out") {
            return "Connection timed out; retrying..."
        }
        return status
    }

    private func sessionActivityUnixMs(_ session: SessionSummary) -> UInt64 {
        max(session.lastEventAtUnixMs, session.createdAtUnixMs)
    }

    private func bumpSessionActivity(sessionId: UUID) {
        guard let index = sessions.firstIndex(where: { $0.id == sessionId }) else {
            return
        }

        let nowMs = UInt64(Date().timeIntervalSince1970 * 1_000)
        if nowMs > sessions[index].lastEventAtUnixMs {
            sessions[index].lastEventAtUnixMs = nowMs
            sessions.sort(by: { sessionActivityUnixMs($0) < sessionActivityUnixMs($1) })
        }
    }

    private func pruneUnavailableSessionsToKnownIDs() {
        let knownSessionIDs = Set(sessions.map(\.id))
        unavailableSessionErrors = unavailableSessionErrors.filter { knownSessionIDs.contains($0.key) }
        unavailableSessionActivitySnapshot = unavailableSessionActivitySnapshot.filter {
            knownSessionIDs.contains($0.key)
        }
    }

    private func clearRecoveredUnavailableSessions() {
        for session in sessions {
            guard let markedActivity = unavailableSessionActivitySnapshot[session.id] else {
                continue
            }

            if sessionActivityUnixMs(session) > markedActivity {
                clearSessionUnavailable(session.id)
            }
        }
    }

    private func clearPendingAttachRequests(for sessionID: UUID) {
        pendingAttachRequestSessionIDs = pendingAttachRequestSessionIDs.filter { $0.value != sessionID }
    }

    private func pruneHistoryPaginationStateToKnownSessionIDs() {
        let knownSessionIDs = Set(sessions.map(\.id))
        hasMoreHistoryBeforeBySessionID = hasMoreHistoryBeforeBySessionID.filter {
            knownSessionIDs.contains($0.key)
        }
        lastHistoryPageRequestAtBySessionID = lastHistoryPageRequestAtBySessionID.filter {
            knownSessionIDs.contains($0.key)
        }
        historyPageLoadInFlightSessionIDs = Set(historyPageLoadInFlightSessionIDs.filter {
            knownSessionIDs.contains($0)
        })
        pendingHistoryPageRequestSessionIDs = pendingHistoryPageRequestSessionIDs.filter {
            knownSessionIDs.contains($0.value)
        }
        historyPageRequestStartedAtByRequestID = historyPageRequestStartedAtByRequestID.filter {
            if let sessionID = pendingHistoryPageRequestSessionIDs[$0.key] {
                return knownSessionIDs.contains(sessionID)
            }
            return false
        }
    }

    private func markSessionUnavailable(_ sessionID: UUID, message: String) {
        unavailableSessionErrors[sessionID] = message
        if let session = sessions.first(where: { $0.id == sessionID }) {
            unavailableSessionActivitySnapshot[sessionID] = sessionActivityUnixMs(session)
        } else {
            unavailableSessionActivitySnapshot[sessionID] = 0
        }
    }

    private func clearSessionUnavailable(_ sessionID: UUID) {
        unavailableSessionErrors.removeValue(forKey: sessionID)
        unavailableSessionActivitySnapshot.removeValue(forKey: sessionID)
    }

    private func preferredAutoSelectedSession(
        in sessions: [SessionSummary],
        excluding excludedSessionIDs: Set<UUID> = []
    ) -> SessionSummary? {
        let eligibleSessions = sessions.filter {
            !excludedSessionIDs.contains($0.id)
                && !isSessionUnavailable($0.id)
                && !isAutoReviewSession($0)
                && !SessionVisibility.isHidden($0)
        }

        if let titled = eligibleSessions.reversed().first(where: hasUsefulAutoSelectionTitle) {
            return titled
        }

        return eligibleSessions.last
    }

    private func hasUsefulAutoSelectionTitle(_ session: SessionSummary) -> Bool {
        guard let rawTitle = session.title?.trimmingCharacters(in: .whitespacesAndNewlines),
              !rawTitle.isEmpty
        else {
            return false
        }

        let normalized = rawTitle.lowercased()
        let lowSignalTitles: Set<String> = [
            "ok",
            "okay",
            "yes",
            "yep",
            "thanks",
            "thank you",
            "go ahead",
            "continue",
            "done",
            "great",
            "cool",
            "test"
        ]

        if lowSignalTitles.contains(normalized) {
            return false
        }

        if normalized.contains("every code harness") {
            return false
        }

        return true
    }

    private func isAutoReviewSession(_ session: SessionSummary) -> Bool {
        let normalized = session.cwd.lowercased()
        return normalized.contains("/.code/working/") && normalized.contains("/branches/auto-review")
    }

}

#if DEBUG
extension SessionMirrorStore {
    func applyEnvelopeForTesting(_ envelope: ServerEnvelope) {
        apply(envelope)
    }

    func recordPendingAttachRequestForTesting(requestId: String, sessionID: UUID) {
        pendingAttachRequestSessionIDs[requestId] = sessionID
    }

    func ingestRawPayloadForTesting(_ payload: String) {
        guard let data = payload.data(using: .utf8) else {
            return
        }
        handleIncomingData(data)
    }
}
#endif

private extension UInt64 {
    func saturatingIncrement() -> UInt64 {
        if self == UInt64.max {
            return self
        }
        return self + 1
    }
}
