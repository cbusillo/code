import Foundation
import OSLog

@MainActor
final class SessionMirrorStore: ObservableObject {
    private static let logger = Logger(subsystem: "com.every.code.native", category: "session-mirror")

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
    private var attachedSessionID: UUID?
    private var expectedAttachedSessionID: UUID?
    private var clientId: String?
    private var userInitiatedDisconnect: Bool = false
    private var requestCounter: UInt64 = 0
    private var attachmentGeneration: UInt64 = 0

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

        let task = URLSession.shared.webSocketTask(with: url)
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
        await send(OutboundMessage.listSessions(requestId: nextRequestID()))
    }

    func createSession(cwd: String?) async {
        let normalized = cwd?.trimmingCharacters(in: .whitespacesAndNewlines)
        let value = normalized?.isEmpty == true ? nil : normalized
        await send(CreateSessionMessage(requestId: nextRequestID(), cwd: value))
    }

    func submitComposer() async {
        guard let selectedSessionID else {
            return
        }

        let text = composerText.trimmingCharacters(in: .whitespacesAndNewlines)
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

        webSocket?.cancel(with: .goingAway, reason: nil)
        webSocket = nil

        attachedSessionID = nil
        expectedAttachedSessionID = nil
        clientId = nil
        attachmentGeneration = attachmentGeneration.saturatingIncrement()
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

        case .sessionList(let message):
            sessions = message.sessions.sorted(by: { sessionActivityUnixMs($0) < sessionActivityUnixMs($1) })

            if let selectedSessionID,
               sessions.contains(where: { $0.id == selectedSessionID }) {
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
            lastError = message.message
            statusLine = "Server error"

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

            guard generation == attachmentGeneration else {
                return
            }
        }

        guard let newSessionID else {
            return
        }

        let fromSeq = itemsBySession[newSessionID]?.last?.seq ?? 0

        guard generation == attachmentGeneration else {
            return
        }

        await send(
            OutboundMessage.attachSession(
                requestId: nextRequestID(),
                sessionId: newSessionID,
                fromSeq: fromSeq
            )
        )
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

    private func preferredAutoSelectedSession(in sessions: [SessionSummary]) -> SessionSummary? {
        sessions.last(where: { !isAutoReviewSession($0) }) ?? sessions.last
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
