import Foundation

@MainActor
final class SessionMirrorStore: ObservableObject {
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

    private var webSocket: URLSessionWebSocketTask?
    private var receiveTask: Task<Void, Never>?
    private var reconnectTask: Task<Void, Never>?
    private var attachedSessionID: UUID?
    private var expectedAttachedSessionID: UUID?
    private var clientID: String?
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

        connectionState = .connecting
        statusLine = "Connecting..."
        lastError = nil
        userInitiatedDisconnect = false
        reconnectTask?.cancel()
        reconnectTask = nil

        let task = URLSession.shared.webSocketTask(with: url)
        task.resume()

        webSocket = task
        connectionState = .connected
        statusLine = "Connected"

        receiveTask = Task {
            await self.receiveLoop()
        }

        await send(OutboundMessage.listSessions(requestID: nextRequestID()))
    }

    func disconnect() {
        userInitiatedDisconnect = true
        cleanupConnection(status: "Disconnected", error: nil)
    }

    func refreshSessions() async {
        await send(OutboundMessage.listSessions(requestID: nextRequestID()))
    }

    func createSession(cwd: String?) async {
        let normalized = cwd?.trimmingCharacters(in: .whitespacesAndNewlines)
        let value = normalized?.isEmpty == true ? nil : normalized
        await send(CreateSessionMessage(requestID: nextRequestID(), cwd: value))
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
                requestID: nextRequestID(),
                sessionID: selectedSessionID,
                text: text,
                cursor: cursor
            )
        )
        await send(SubmitTurnMessage(requestID: nextRequestID(), sessionID: selectedSessionID))
        composerText = ""
    }

    func interruptTurn() async {
        guard let selectedSessionID else {
            return
        }
        await send(InterruptTurnMessage(requestID: nextRequestID(), sessionID: selectedSessionID))
    }

    func submitApproval(callID: String, type: ApprovalType, approved: Bool) async {
        guard let selectedSessionID else {
            return
        }

        let decision = approved ? "approved" : "denied"
        switch type {
        case .exec:
            await send(
                ExecApprovalMessage(
                    requestID: nextRequestID(),
                    sessionID: selectedSessionID,
                    callID: callID,
                    decision: decision
                )
            )
        case .patch:
            await send(
                PatchApprovalMessage(
                    requestID: nextRequestID(),
                    sessionID: selectedSessionID,
                    callID: callID,
                    decision: decision
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
        clientID = nil
        attachmentGeneration = attachmentGeneration.saturatingIncrement()
        if error != nil {
            lastError = error
        }
        connectionState = .disconnected
        statusLine = status

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
            lastError = "Failed to decode server message: \(error.localizedDescription)"
        }
    }

    private func apply(_ envelope: ServerEnvelope) {
        switch envelope {
        case .hello(let message):
            clientID = message.clientID
            statusLine = "Connected as \(message.clientID.prefix(8))"

        case .sessionList(let message):
            sessions = message.sessions.sorted(by: { $0.createdAtUnixMs < $1.createdAtUnixMs })

            if let selectedSessionID,
               sessions.contains(where: { $0.id == selectedSessionID }) {
                return
            }

            selectedSessionID = sessions.last?.id

        case .sessionCreated(let message):
            upsertSession(message.session)
            selectedSessionID = message.session.id

        case .sessionAttached(let message):
            let existing = itemsBySession[message.sessionID] ?? []
            let merged = SessionStreamReducer.mergeReplayItems(
                existing: existing,
                incoming: message.items,
                fromSeq: message.fromSeq
            )
            itemsBySession[message.sessionID] = merged

            if !SessionStreamReducer.shouldAcceptSessionAttached(
                selectedSessionID: selectedSessionID,
                expectedSessionID: expectedAttachedSessionID,
                attachedSessionID: message.sessionID
            ) {
                return
            }

            attachedSessionID = message.sessionID
            expectedAttachedSessionID = nil
            statusLine = "Attached to \(message.sessionID.uuidString.prefix(8))"

        case .sessionDetached(let message):
            if attachedSessionID == message.sessionID {
                attachedSessionID = nil
            }
            statusLine = "Detached from \(message.sessionID.uuidString.prefix(8))"

        case .sessionStream(let message):
            let sessionID = message.item.sessionID
            itemsBySession[sessionID] = SessionStreamReducer.appendLiveItem(
                items: itemsBySession[sessionID] ?? [],
                newItem: message.item
            )

            if sessionID == selectedSessionID,
               message.item.type == "composer" {
                let isFromCurrentClient = message.item.sourceClientID == clientID
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
            sessions.sort(by: { $0.createdAtUnixMs < $1.createdAtUnixMs })
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
            await send(OutboundMessage.detachSession(requestID: nextRequestID(), sessionID: oldSessionID))
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
                requestID: nextRequestID(),
                sessionID: newSessionID,
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
            lastError = "Failed to send message: \(error.localizedDescription)"
        }
    }

    private func nextRequestID() -> String {
        requestCounter = requestCounter.saturatingIncrement()
        return "native_\(requestCounter)"
    }

}

private extension UInt64 {
    func saturatingIncrement() -> UInt64 {
        if self == UInt64.max {
            return self
        }
        return self + 1
    }
}
