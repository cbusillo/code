import Foundation
import OSLog

@MainActor
final class SessionMirrorStore: ObservableObject {
    private static let logger = Logger(subsystem: "com.every.code.native", category: "session-mirror")
    static let defaultEndpoint = "ws://127.0.0.1:4317/ws"
    nonisolated static let companionPairingScheme = "ecccompanion"
    nonisolated static let companionPairingHost = "pair"
    // Large catalogs can contain hundreds of sessions; keep periodic refreshes
    // sparse so list decoding doesn't interrupt typing responsiveness.
    private static let catalogRefreshIntervalNanoseconds: UInt64 = 15_000_000_000
    private static let sessionListRequestTimeoutSeconds: TimeInterval = 20
    private static let historyPageRequestThrottleSeconds: TimeInterval = 0.35
    private static let slowHistoryPageThresholdMs: Double = 350
    private static let inactiveSessionItemRetentionLimit = 1_500

    struct CompanionPairingCodePayload: Equatable {
        let endpoint: String
        let token: String
        let deviceID: String?
        let expiresAtUnixMs: UInt64?
    }

    struct HistoryPageTelemetry {
        var requestCount: UInt64 = 0
        var successCount: UInt64 = 0
        // Counts pages likely to hitch rendering; this is a proxy, not actual dropped-frame telemetry.
        var slowPageCount: UInt64 = 0
        var totalLatencyMs: Double = 0
        var inactiveRetentionPasses: UInt64 = 0
        var inactiveRetentionTrimmedItems: UInt64 = 0

        var averageLatencyMs: Double {
            guard successCount > 0 else {
                return 0
            }
            return totalLatencyMs / Double(successCount)
        }
    }

    enum ConnectionState: String, Decodable {
        case disconnected
        case connecting
        case connected
    }

    enum SessionRuntimeState: String {
        case connected
        case reconnecting
        case historyLoading
        case historyComplete
        case unavailable

        var label: String {
            switch self {
            case .connected:
                return "Connected"
            case .reconnecting:
                return "Reconnecting"
            case .historyLoading:
                return "History loading"
            case .historyComplete:
                return "History complete"
            case .unavailable:
                return "Unavailable"
            }
        }
    }

    enum EndpointAccessPolicy {
        case loopbackOnly
        case anyHost

        var rejectionStatusLine: String {
            switch self {
            case .loopbackOnly:
                return "Loopback endpoints only"
            case .anyHost:
                return "Unsupported endpoint"
            }
        }

        var rejectionMessage: String {
            switch self {
            case .loopbackOnly:
                return "Endpoint must use ws://localhost, ws://127.0.0.1, or ws://[::1]."
            case .anyHost:
                return "Endpoint must use ws:// or wss:// with a host name."
            }
        }
    }

    @Published var endpoint: String
    @Published private(set) var connectionState: ConnectionState = .disconnected
    @Published private(set) var statusLine: String = "Disconnected"
    @Published private(set) var sessions: [SessionSummary] = []
    @Published private(set) var availableModelOptions: [WorkflowModelOption] = []
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
            if suppressSelectionSideEffects {
                return
            }
            enforceHistoryRetentionPolicy()
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
    @Published var companionSessionToken: String?
    @Published var companionLANEndpoint: String?

    let endpointAccessPolicy: EndpointAccessPolicy

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
    private var suppressSelectionSideEffects: Bool = false
    private var loadedBenchmarkFixturePath: String?

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

    init(
        initialEndpoint: String = SessionMirrorStore.defaultEndpoint,
        companionSessionToken: String? = nil,
        companionLANEndpoint: String? = nil,
        endpointAccessPolicy: EndpointAccessPolicy = .loopbackOnly
    ) {
        endpoint = initialEndpoint
        self.companionSessionToken = companionSessionToken
        self.companionLANEndpoint = companionLANEndpoint
        self.endpointAccessPolicy = endpointAccessPolicy
    }

    var selectedSession: SessionSummary? {
        guard let selectedSessionID else {
            return nil
        }
        return sessions.first(where: { $0.id == selectedSessionID })
    }

    var selectedSessionRuntimeState: SessionRuntimeState {
        runtimeState(for: selectedSessionID)
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

    var companionPairingCode: String? {
        guard let endpoint = preferredCompanionPairingEndpoint(),
              let companionSessionToken = normalizedCompanionSessionToken()
        else {
            return nil
        }

        return Self.buildCompanionPairingCode(endpoint: endpoint, token: companionSessionToken)
    }

    func runtimeState(for sessionID: UUID?) -> SessionRuntimeState {
        if let sessionID,
           isSessionUnavailable(sessionID) {
            return .unavailable
        }

        if connectionState != .connected {
            return .reconnecting
        }

        guard let sessionID,
              let items = itemsBySession[sessionID],
              !items.isEmpty
        else {
            return .connected
        }

        if historyPageLoadInFlightSessionIDs.contains(sessionID) {
            return .historyLoading
        }

        if hasMoreHistoryBefore(sessionID) == false {
            return .historyComplete
        }

        return .connected
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

        guard Self.endpointIsAllowed(url, policy: endpointAccessPolicy) else {
            statusLine = endpointAccessPolicy.rejectionStatusLine
            lastError = endpointAccessPolicy.rejectionMessage
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

        var request = URLRequest(url: url)
        if let companionSessionToken,
           !companionSessionToken.isEmpty {
            request.setValue("Bearer \(companionSessionToken)", forHTTPHeaderField: "Authorization")
        }

        let task = URLSession.shared.webSocketTask(with: request)
        task.maximumMessageSize = 8_000_000
        task.resume()

        webSocket = task

        receiveTask = Task {
            await self.receiveLoop()
        }
    }

    @discardableResult
    func importCompanionPairingCode(_ raw: String) -> Bool {
        guard let payload = Self.parseCompanionPairingCode(raw) else {
            lastError = "Pairing code is invalid."
            statusLine = "Pairing code invalid"
            return false
        }

        if let expiresAtUnixMs = payload.expiresAtUnixMs,
           Self.currentUnixTimeMilliseconds() >= expiresAtUnixMs {
            lastError = "Pairing code has expired. Request a fresh code from your Mac companion."
            statusLine = "Pairing code expired"
            return false
        }

        guard let endpointURL = URL(string: payload.endpoint),
              Self.endpointIsAllowed(endpointURL, policy: endpointAccessPolicy)
        else {
            lastError = endpointAccessPolicy.rejectionMessage
            statusLine = endpointAccessPolicy.rejectionStatusLine
            return false
        }

        endpoint = payload.endpoint
        companionSessionToken = payload.token
        statusLine = payload.deviceID.map { "Paired: \($0)" } ?? "Pairing imported"
        lastError = nil
        return true
    }

    func loadBenchmarkFixture(from path: String) {
        guard loadedBenchmarkFixturePath != path else {
            return
        }

        let fixtureURL = URL(fileURLWithPath: path)
        do {
            let data = try Data(contentsOf: fixtureURL)
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            let fixture = try decoder.decode(BenchmarkFixture.self, from: data)
            applyBenchmarkFixture(fixture)
            loadedBenchmarkFixturePath = path
            Self.logger.info("Loaded benchmark fixture from \(path, privacy: .public)")
        } catch {
            loadedBenchmarkFixturePath = nil
            lastError = "Failed to load benchmark fixture: \(error.localizedDescription)"
            statusLine = "Fixture load failed"
            Self.logger.error("Failed to load benchmark fixture from \(path, privacy: .public): \(error.localizedDescription, privacy: .public)")
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

    func createSession(
        cwd: String?,
        model: String? = nil,
        reasoningEffort: String? = nil
    ) async {
        let normalized = cwd?.trimmingCharacters(in: .whitespacesAndNewlines)
        let value = normalized?.isEmpty == true ? nil : normalized
        await send(
            CreateSessionMessage(
                requestId: nextRequestID(),
                cwd: value,
                model: model,
                reasoningEffort: reasoningEffort
            )
        )
    }

    func submitComposer(
        text explicitText: String? = nil,
        model: String? = nil,
        reasoningEffort: String? = nil
    ) async {
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
        await send(
            SubmitTurnMessage(
                requestId: nextRequestID(),
                sessionId: selectedSessionID,
                model: model,
                reasoningEffort: reasoningEffort
            )
        )
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

    func submitRequestUserInput(
        sessionId: UUID,
        turnId: String,
        answersByQuestionID: [String: [String]]
    ) async {
        let normalizedTurnID = turnId.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalizedTurnID.isEmpty else {
            return
        }

        let payload = answersByQuestionID.reduce(into: [String: UserInputAnswerPayload]()) { partial, entry in
            let values = entry.value
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
            partial[entry.key] = UserInputAnswerPayload(answers: values)
        }

        await send(
            UserInputAnswerMessage(
                requestId: nextRequestID(),
                sessionId: sessionId,
                turnId: normalizedTurnID,
                answers: payload
            )
        )
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
                await self.send(OutboundMessage.listModels(requestId: self.nextRequestID()))
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

        case .modelList(let message):
            availableModelOptions = message.data

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
            enforceHistoryRetentionPolicy()
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
            guard consumeHistoryPageResponseTracking(for: message) else {
                return
            }

            let existing = itemsBySession[message.sessionId] ?? []
            let merged = SessionStreamReducer.mergeOlderHistoryPage(
                existing: existing,
                incoming: message.items,
                beforeSeq: message.beforeSeq
            )
            itemsBySession[message.sessionId] = merged
            hasMoreHistoryBeforeBySessionID[message.sessionId] = message.hasMoreBefore
            enforceHistoryRetentionPolicy()

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
            enforceHistoryRetentionPolicy()

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

    func requestOlderHistoryIfNeeded(for sessionID: UUID) -> Bool {
        guard connectionState == .connected else {
            return false
        }

        guard let oldestSeq = itemsBySession[sessionID]?.first?.seq else {
            return false
        }

        if oldestSeq <= 1 {
            hasMoreHistoryBeforeBySessionID[sessionID] = false
            return false
        }

        if historyPageLoadInFlightSessionIDs.contains(sessionID) {
            return false
        }

        if let lastRequestAt = lastHistoryPageRequestAtBySessionID[sessionID],
           Date().timeIntervalSince(lastRequestAt) < Self.historyPageRequestThrottleSeconds {
            return false
        }

        if hasMoreHistoryBeforeBySessionID[sessionID] == false {
            return false
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

        return true
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

    private func send(_ message: UserInputAnswerMessage) async {
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

    nonisolated static func endpointIsAllowed(_ url: URL, policy: EndpointAccessPolicy) -> Bool {
        guard let scheme = url.scheme?.lowercased(), scheme == "ws" || scheme == "wss" else {
            return false
        }

        guard let host = url.host?.lowercased() else {
            return false
        }

        switch policy {
        case .anyHost:
            return true
        case .loopbackOnly:
            break
        }

        return host == "localhost" || host == "127.0.0.1" || host == "::1"
    }

    nonisolated static func buildCompanionPairingCode(
        endpoint: String,
        token: String,
        deviceID: String? = nil,
        expiresAtUnixMs: UInt64? = nil
    ) -> String? {
        var components = URLComponents()
        components.scheme = companionPairingScheme
        components.host = companionPairingHost
        var queryItems = [
            URLQueryItem(name: "endpoint", value: endpoint),
            URLQueryItem(name: "token", value: token),
        ]
        if let deviceID,
           !deviceID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            queryItems.append(URLQueryItem(name: "device_id", value: deviceID))
        }
        if let expiresAtUnixMs {
            queryItems.append(
                URLQueryItem(name: "expires_at_unix_ms", value: "\(expiresAtUnixMs)")
            )
        }
        components.queryItems = queryItems
        return components.string
    }

    nonisolated static func parseCompanionPairingCode(_ raw: String) -> CompanionPairingCodePayload? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              let components = URLComponents(string: trimmed),
              components.scheme?.lowercased() == companionPairingScheme,
              components.host?.lowercased() == companionPairingHost
        else {
            return nil
        }

        let queryItems = components.queryItems ?? []
        let endpoint = queryItems.first(where: { $0.name == "endpoint" })?.value?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let token = queryItems.first(where: { $0.name == "token" })?.value?
            .trimmingCharacters(in: .whitespacesAndNewlines)

        guard let endpoint,
              !endpoint.isEmpty,
              let token,
              !token.isEmpty
        else {
            return nil
        }

        let deviceID = queryItems
            .first(where: { $0.name == "device_id" || $0.name == "device" })?
            .value?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let normalizedDeviceID: String?
        if let deviceID,
           !deviceID.isEmpty {
            normalizedDeviceID = deviceID
        } else {
            normalizedDeviceID = nil
        }

        let expiresAtUnixMs = queryItems
            .first(where: { $0.name == "expires_at_unix_ms" || $0.name == "expires_at" })?
            .value
            .flatMap(UInt64.init)

        return CompanionPairingCodePayload(
            endpoint: endpoint,
            token: token,
            deviceID: normalizedDeviceID,
            expiresAtUnixMs: expiresAtUnixMs
        )
    }

    nonisolated static func currentUnixTimeMilliseconds() -> UInt64 {
        UInt64(Date().timeIntervalSince1970 * 1_000)
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
        Self.disconnectStatusLine(status: status, error: error)
    }

    nonisolated static func disconnectStatusLine(status: String, error: String?) -> String {
        guard let error else {
            return status
        }

        let normalized = error.lowercased()
        if normalized.contains("401") || normalized.contains("unauthorized") {
            return "Pair required; update companion token."
        }
        if normalized.contains("could not connect") || normalized.contains("not connected") {
            return "Backend offline; retrying..."
        }
        if normalized.contains("timed out") {
            return "Connection timed out; retrying..."
        }
        return status
    }

    private func preferredCompanionPairingEndpoint() -> String? {
        if let companionLANEndpoint {
            let trimmedLANEndpoint = companionLANEndpoint.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmedLANEndpoint.isEmpty {
                return trimmedLANEndpoint
            }
        }

        let trimmedEndpoint = endpoint.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedEndpoint.isEmpty {
            return nil
        }
        return trimmedEndpoint
    }

    private func normalizedCompanionSessionToken() -> String? {
        guard let companionSessionToken else {
            return nil
        }

        let trimmedToken = companionSessionToken.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedToken.isEmpty {
            return nil
        }
        return trimmedToken
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

    private func enforceHistoryRetentionPolicy() {
        guard !itemsBySession.isEmpty else {
            return
        }

        var retained = itemsBySession
        var changed = false

        for (sessionID, items) in itemsBySession {
            guard sessionID != selectedSessionID,
                  items.count > Self.inactiveSessionItemRetentionLimit
            else {
                continue
            }

            let trimmed = Array(items.suffix(Self.inactiveSessionItemRetentionLimit))
            if trimmed.count != items.count {
                retained[sessionID] = trimmed
                changed = true
                historyPageTelemetry.inactiveRetentionPasses = historyPageTelemetry.inactiveRetentionPasses.saturatingIncrement()
                let trimmedCount = UInt64(items.count - trimmed.count)
                historyPageTelemetry.inactiveRetentionTrimmedItems = historyPageTelemetry.inactiveRetentionTrimmedItems.saturatingAdding(trimmedCount)

                if let firstSeq = trimmed.first?.seq,
                   firstSeq > 1 {
                    hasMoreHistoryBeforeBySessionID[sessionID] = true
                }
            }
        }

        if changed {
            itemsBySession = retained
        }
    }

    private func consumeHistoryPageResponseTracking(
        for message: SessionHistoryPageMessage
    ) -> Bool {
        guard let requestId = message.requestId else {
            historyPageLoadInFlightSessionIDs.remove(message.sessionId)
            return true
        }

        guard let pendingSessionID = pendingHistoryPageRequestSessionIDs.removeValue(forKey: requestId) else {
            historyPageRequestStartedAtByRequestID.removeValue(forKey: requestId)
            clearOrphanedHistoryPageInFlightState(for: message.sessionId)
            Self.logger.debug(
                "Ignoring stale history page response request_id=\(requestId, privacy: .public) for session \(message.sessionId.uuidString, privacy: .public)"
            )
            return false
        }

        let startedAt = historyPageRequestStartedAtByRequestID.removeValue(forKey: requestId)
        historyPageLoadInFlightSessionIDs.remove(pendingSessionID)

        guard pendingSessionID == message.sessionId else {
            clearOrphanedHistoryPageInFlightState(for: message.sessionId)
            Self.logger.warning(
                "Ignoring mismatched history page response request_id=\(requestId, privacy: .public) expected_session=\(pendingSessionID.uuidString, privacy: .public) actual_session=\(message.sessionId.uuidString, privacy: .public)"
            )
            return false
        }

        if let startedAt {
            let latencyMs = Date().timeIntervalSince(startedAt) * 1_000
            historyPageTelemetry.successCount += 1
            historyPageTelemetry.totalLatencyMs += latencyMs
            if latencyMs > Self.slowHistoryPageThresholdMs {
                historyPageTelemetry.slowPageCount += 1
            }
        }

        return true
    }

    private func clearOrphanedHistoryPageInFlightState(for sessionID: UUID) {
        if pendingHistoryPageRequestSessionIDs.values.contains(sessionID) {
            return
        }
        historyPageLoadInFlightSessionIDs.remove(sessionID)
    }

    private func applyBenchmarkFixture(_ fixture: BenchmarkFixture) {
        receiveTask?.cancel()
        receiveTask = nil
        reconnectTask?.cancel()
        reconnectTask = nil
        catalogRefreshTask?.cancel()
        catalogRefreshTask = nil
        webSocket?.cancel(with: .goingAway, reason: nil)
        webSocket = nil

        if let endpoint = fixture.endpoint,
           !endpoint.isEmpty {
            self.endpoint = endpoint
        }
        availableModelOptions = fixture.modelOptions ?? []

        userInitiatedDisconnect = true
        expectedAttachedSessionID = nil
        pendingAttachRequestSessionIDs.removeAll()
        pendingHistoryPageRequestSessionIDs.removeAll()
        historyPageRequestStartedAtByRequestID.removeAll()
        lastHistoryPageRequestAtBySessionID.removeAll()
        hasMoreHistoryBeforeBySessionID.removeAll()
        unavailableSessionActivitySnapshot.removeAll()
        sessionListRequestID = nil
        sessionListRequestStartedAt = nil

        var builtSessions: [SessionSummary] = []
        var builtItemsBySession: [UUID: [SessionStreamItem]] = [:]
        var builtHasMoreBySessionID: [UUID: Bool] = [:]
        var builtUnavailableSessionErrors: [UUID: String] = [:]
        var builtInFlightHistorySessionIDs: Set<UUID> = []

        for sessionFixture in fixture.sessions {
            let sessionID = sessionFixture.summary.id
            builtSessions.append(sessionFixture.summary)
            builtItemsBySession[sessionID] = SessionStreamReducer.mergeReplayItems(
                existing: [],
                incoming: sessionFixture.items,
                fromSeq: 0
            )

            if let hasMoreHistoryBefore = sessionFixture.hasMoreHistoryBefore {
                builtHasMoreBySessionID[sessionID] = hasMoreHistoryBefore
            }

            if sessionFixture.historyPageInFlight == true {
                builtInFlightHistorySessionIDs.insert(sessionID)
            }

            if let unavailableError = sessionFixture.unavailableError?.trimmingCharacters(in: .whitespacesAndNewlines),
               !unavailableError.isEmpty {
                builtUnavailableSessionErrors[sessionID] = unavailableError
            }
        }

        sessions = builtSessions.sorted(by: { sessionActivityUnixMs($0) < sessionActivityUnixMs($1) })
        itemsBySession = builtItemsBySession
        hasMoreHistoryBeforeBySessionID = builtHasMoreBySessionID
        unavailableSessionErrors = builtUnavailableSessionErrors

        if let historyPageInFlightSessionIDs = fixture.historyPageInFlightSessionIDs {
            builtInFlightHistorySessionIDs.formUnion(historyPageInFlightSessionIDs)
        }
        historyPageLoadInFlightSessionIDs = builtInFlightHistorySessionIDs

        if let telemetry = fixture.historyPageTelemetry {
            historyPageTelemetry = HistoryPageTelemetry(
                requestCount: telemetry.requestCount,
                successCount: telemetry.successCount,
                slowPageCount: telemetry.slowPageCount,
                totalLatencyMs: telemetry.totalLatencyMs,
                inactiveRetentionPasses: telemetry.inactiveRetentionPasses,
                inactiveRetentionTrimmedItems: telemetry.inactiveRetentionTrimmedItems
            )
        } else {
            historyPageTelemetry = HistoryPageTelemetry()
        }

        suppressSelectionSideEffects = true
        selectedSessionID = fixture.selectedSessionID ?? sessions.last?.id
        suppressSelectionSideEffects = false

        attachedSessionID = selectedSessionID
        expectedAttachedSessionID = nil

        composerText = fixture.composerText ?? ""
        companionSessionToken = fixture.companionSessionToken
        companionLANEndpoint = fixture.companionLanEndpoint
        connectionState = fixture.connectionState
        statusLine = fixture.statusLine ?? fixture.connectionState.defaultStatusLine
        lastError = fixture.lastError
        clientId = fixture.clientId
        transportFailureCount = 0
        lastTransportFailureAt = nil

        enforceHistoryRetentionPolicy()
    }

    private func markSessionUnavailable(_ sessionID: UUID, message: String) {
        unavailableSessionErrors[sessionID] = normalizedUnavailableSessionMessage(message)
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

    private func normalizedUnavailableSessionMessage(_ message: String) -> String {
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return message
        }

        let normalized = trimmed.lowercased()
        if normalized.contains("already active in another code runtime") {
            return "This thread is active in another Code runtime. Close the other runtime for this thread, then retry attach."
        }

        return trimmed
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

private struct BenchmarkFixture: Decodable {
    let endpoint: String?
    let modelOptions: [WorkflowModelOption]?
    let connectionState: SessionMirrorStore.ConnectionState
    let statusLine: String?
    let lastError: String?
    let clientId: String?
    let companionSessionToken: String?
    let companionLanEndpoint: String?
    let composerText: String?
    let selectedSessionID: UUID?
    let sessions: [BenchmarkFixtureSession]
    let historyPageInFlightSessionIDs: [UUID]?
    let historyPageTelemetry: BenchmarkHistoryPageTelemetry?
}

private struct BenchmarkFixtureSession: Decodable {
    let summary: SessionSummary
    let items: [SessionStreamItem]
    let hasMoreHistoryBefore: Bool?
    let historyPageInFlight: Bool?
    let unavailableError: String?
}

private struct BenchmarkHistoryPageTelemetry: Decodable {
    let requestCount: UInt64
    let successCount: UInt64
    let slowPageCount: UInt64
    let totalLatencyMs: Double
    let inactiveRetentionPasses: UInt64
    let inactiveRetentionTrimmedItems: UInt64

    private enum CodingKeys: String, CodingKey {
        case requestCount
        case successCount
        case slowPageCount
        case totalLatencyMs
        case inactiveRetentionPasses
        case inactiveRetentionTrimmedItems
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        requestCount = try container.decodeIfPresent(UInt64.self, forKey: .requestCount) ?? 0
        successCount = try container.decodeIfPresent(UInt64.self, forKey: .successCount) ?? 0
        slowPageCount = try container.decodeIfPresent(UInt64.self, forKey: .slowPageCount) ?? 0
        totalLatencyMs = try container.decodeIfPresent(Double.self, forKey: .totalLatencyMs) ?? 0
        inactiveRetentionPasses = try container.decodeIfPresent(UInt64.self, forKey: .inactiveRetentionPasses) ?? 0
        inactiveRetentionTrimmedItems = try container.decodeIfPresent(UInt64.self, forKey: .inactiveRetentionTrimmedItems) ?? 0
    }
}

private extension SessionMirrorStore.ConnectionState {
    var defaultStatusLine: String {
        switch self {
        case .disconnected:
            return "Disconnected"
        case .connecting:
            return "Connecting..."
        case .connected:
            return "Connected"
        }
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

    func recordPendingHistoryPageRequestForTesting(requestId: String, sessionID: UUID) {
        pendingHistoryPageRequestSessionIDs[requestId] = sessionID
        historyPageLoadInFlightSessionIDs.insert(sessionID)
        historyPageRequestStartedAtByRequestID[requestId] = Date()
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

    func saturatingAdding(_ value: UInt64) -> UInt64 {
        let (sum, overflow) = addingReportingOverflow(value)
        return overflow ? UInt64.max : sum
    }
}
