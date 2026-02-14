import Foundation

struct SessionSummary: Decodable, Hashable, Identifiable {
    let id: UUID
    let conversationId: String
    let model: String
    let cwd: String
    let createdAtUnixMs: UInt64
    var lastEventAtUnixMs: UInt64
    let title: String?

    private enum CodingKeys: String, CodingKey {
        case id
        case conversationId
        case model
        case cwd
        case createdAtUnixMs
        case lastEventAtUnixMs
        case title
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(UUID.self, forKey: .id)
        conversationId = try container.decode(String.self, forKey: .conversationId)
        model = try container.decode(String.self, forKey: .model)
        cwd = try container.decode(String.self, forKey: .cwd)
        createdAtUnixMs = try container.decode(UInt64.self, forKey: .createdAtUnixMs)
        lastEventAtUnixMs = try container.decodeIfPresent(UInt64.self, forKey: .lastEventAtUnixMs)
            ?? createdAtUnixMs
        title = try container.decodeIfPresent(String.self, forKey: .title)
    }
}

struct SessionAttachedMessage: Decodable {
    let sessionId: UUID
    let fromSeq: UInt64
    let items: [SessionStreamItem]
}

struct SessionStreamMessage: Decodable {
    let item: SessionStreamItem
}

struct SessionCreatedMessage: Decodable {
    let session: SessionSummary
}

struct SessionDetachedMessage: Decodable {
    let sessionId: UUID
}

struct SessionListMessage: Decodable {
    let sessions: [SessionSummary]
}

struct HelloMessage: Decodable {
    let clientId: String
}

struct ErrorMessage: Decodable {
    let requestId: String?
    let message: String
}

struct SessionStreamItem: Decodable, Hashable, Identifiable {
    private static let tokenNumberFormatter: NumberFormatter = {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        return formatter
    }()

    private static let compactTokenNumberFormatter: NumberFormatter = {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.maximumFractionDigits = 1
        formatter.minimumFractionDigits = 0
        return formatter
    }()

    let type: String
    let sessionId: UUID
    let seq: UInt64
    let event: CoreEventPayload?
    let rev: UInt64?
    let text: String?
    let cursor: Int?
    let sourceClientId: String?
    let level: String?
    let message: String?

    var id: String {
        "\(sessionId.uuidString)-\(seq)-\(type)"
    }

    var title: String {
        switch type {
        case "core_event":
            switch event?.payload?.typeHint {
            case "turn_diff":
                return "diff"
            case "token_count":
                return "token usage"
            case "exec_command_end":
                return "ran command"
            case "exec_command_begin":
                return "running command"
            case "patch_apply_end":
                return "applied patch"
            case "agent_reasoning", "agent_reasoning_section_break":
                return "reasoning"
            case "agent_message":
                return "assistant"
            case "user_message":
                return "user"
            case let value?:
                return value.replacingOccurrences(of: "_", with: " ")
            case nil:
                return event?.kind ?? type
            }
        case "composer":
            return "composer"
        case "system":
            return level ?? "system"
        default:
            return type
        }
    }

    var body: String {
        switch type {
        case "core_event":
            return coreEventBody
        case "composer":
            return text ?? ""
        case "system":
            return message ?? ""
        default:
            return message ?? ""
        }
    }

    private var coreEventBody: String {
        guard let payload = event?.payload else {
            return "(empty core payload)"
        }

        guard let object = payload.objectValue else {
            return payload.pretty
        }

        let payloadType = object["type"]?.stringValue ?? payload.typeHint ?? ""

        if payloadType == "agent_message",
           let message = object["message"]?.stringValue {
            return normalizeStructuredText(message)
        }

        if payloadType == "user_message",
           let message = object["message"]?.stringValue {
            if isImagePlaceholderMessage(message) {
                return "Shared image"
            }
            return normalizeStructuredText(message)
        }

        if payloadType == "replay_history" {
            return summarizeReplayHistory(object)
        }

        if payloadType == "entered_review_mode",
           let prompt = object["prompt"]?.stringValue {
            return truncate(normalizeStructuredText(prompt), maxCharacters: 900)
        }

        if payloadType == "turn_diff",
           let unifiedDiff = object["unified_diff"]?.stringValue {
            return summarizeTurnDiff(unifiedDiff)
        }

        if payloadType == "agent_reasoning",
           let reasoning = object["text"]?.stringValue {
            return truncate(normalizeReasoningText(reasoning), maxCharacters: 3_000)
        }

        if payloadType == "agent_reasoning_section_break" {
            return "Reasoning section"
        }

        if payloadType == "token_count" {
            return summarizeTokenCount(object)
        }

        if payloadType == "exec_command_begin" {
            return summarizeExecCommandBegin(object)
        }

        if payloadType == "exec_command_end" {
            return summarizeExecCommandEnd(object)
        }

        if payloadType == "patch_apply_end" {
            return summarizePatchApplyEnd(object)
        }

        if payloadType == "turn_aborted" {
            return summarizeTurnAborted(object)
        }

        if payloadType == "session_attached" {
            return summarizeSessionAttached(object)
        }

        if payloadType == "session_detached" {
            return "Detached from thread"
        }

        if let message = object["message"]?.stringValue {
            return truncate(normalizeStructuredText(message), maxCharacters: 8_000)
        }

        if payloadType.contains("diff") || payloadType.contains("delta") {
            return truncate(payload.pretty, maxCharacters: 2_500)
        }

        return truncate(payload.pretty, maxCharacters: 6_000)
    }

    private func summarizeTurnDiff(_ value: String) -> String {
        let diff = normalizeStructuredText(value)
        let fileCount = countGitDiffHeaders(in: diff)

        let header = fileCount > 0 ? "\(fileCount) file\(fileCount == 1 ? "" : "s") changed\n\n" : ""
        let bodyLimit = 1_400
        let truncated = truncate(diff, maxCharacters: bodyLimit)
        return "\(header)\(truncated)"
    }

    private func summarizeTokenCount(_ object: [String: JSONValue]) -> String {
        guard let info = object["info"]?.objectValue else {
            return "Token usage updated"
        }

        let requestedModel = info["requested_model"]?.stringValue ?? "current model"
        let model = formatModelName(requestedModel)
        let usage = info["last_token_usage"]?.objectValue
        let total = usage?["total_tokens"]?.numberValue.map(Int.init)
        let input = usage?["input_tokens"]?.numberValue.map(Int.init)
        let output = usage?["output_tokens"]?.numberValue.map(Int.init)
        let reasoning = usage?["reasoning_output_tokens"]?.numberValue.map(Int.init)

        var parts: [String] = [model]
        if let total {
            parts.append("\(formatCompactTokenCount(total)) total")
        }
        if let input {
            parts.append("\(formatCompactTokenCount(input)) in")
        }
        if let output {
            parts.append("\(formatCompactTokenCount(output)) out")
        }
        if let reasoning {
            parts.append("\(formatCompactTokenCount(reasoning)) reasoning")
        }

        return parts.joined(separator: " · ")
    }

    private func summarizeExecCommandBegin(_ object: [String: JSONValue]) -> String {
        let command = joinedCommand(from: object["command"])
        if command.isEmpty {
            return "Running command"
        }
        return "Running `\(command)`"
    }

    private func summarizeExecCommandEnd(_ object: [String: JSONValue]) -> String {
        let command = joinedCommand(from: object["command"])
        let exitCode = object["exit_code"]?.numberValue.map(Int.init)
        let duration = object["duration"]?.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines)
        let successLabel: String
        if let exitCode {
            successLabel = exitCode == 0 ? "Success" : "Failed (\(exitCode))"
        } else {
            successLabel = "Completed"
        }

        var headlineParts: [String] = [successLabel]
        if !command.isEmpty {
            headlineParts.append("`\(command)`")
        }
        if let duration,
           !duration.isEmpty {
            headlineParts.append(duration)
        }

        let outputCandidate = object["formatted_output"]?.stringValue
            ?? object["aggregated_output"]?.stringValue
            ?? object["stdout"]?.stringValue
            ?? object["stderr"]?.stringValue

        let output = outputCandidate
            .map(normalizeStructuredText)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            ?? ""

        if output.isEmpty {
            return headlineParts.joined(separator: " · ")
        }

        let preview = output
            .components(separatedBy: "\n")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
            .prefix(5)
            .joined(separator: "\n")

        return "\(headlineParts.joined(separator: " · "))\n\(truncate(preview, maxCharacters: 380))"
    }

    private func summarizeTurnAborted(_ object: [String: JSONValue]) -> String {
        if let reason = object["reason"]?.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines),
           !reason.isEmpty {
            return "Reason: \(reason.replacingOccurrences(of: "_", with: " "))"
        }

        return "Turn stopped"
    }

    private func summarizeSessionAttached(_ object: [String: JSONValue]) -> String {
        let fromSeq = object["from_seq"]?.numberValue.map(UInt64.init)
        let replayCount = object["replay_item_count"]?.numberValue.map(Int.init)

        var details: [String] = []
        if let fromSeq {
            details.append("from seq \(fromSeq)")
        }
        if let replayCount {
            details.append("\(replayCount) replay item\(replayCount == 1 ? "" : "s")")
        }

        if details.isEmpty {
            return "Attached to thread"
        }

        return "Attached to thread (\(details.joined(separator: ", ")))"
    }

    private func formatModelName(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return value
        }

        var formatted = trimmed
        if formatted.lowercased().hasPrefix("gpt-") {
            formatted = "GPT-\(formatted.dropFirst(4))"
        }

        formatted = formatted.replacingOccurrences(of: "-codex", with: "-Codex")
        formatted = formatted.replacingOccurrences(of: "-mini", with: "-Mini")
        return formatted
    }

    private func summarizePatchApplyEnd(_ object: [String: JSONValue]) -> String {
        let success = object["success"]?.boolValue ?? false
        let statusLine = success ? "Applied changes" : "Patch failed"

        let stdout = object["stdout"]?.stringValue
            .map(normalizeStructuredText)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }

        var changedFiles: [String] = []
        if let stdout {
            for line in stdout.components(separatedBy: "\n") {
                let trimmed = line.trimmingCharacters(in: .whitespaces)
                guard trimmed.count > 2 else {
                    continue
                }

                let marker = trimmed.prefix(2)
                if marker == "M " || marker == "A " || marker == "D " || marker == "R " {
                    changedFiles.append(compactDisplayPath(String(trimmed.dropFirst(2))))
                    continue
                }

                if looksLikePathLine(trimmed) {
                    changedFiles.append(compactDisplayPath(trimmed))
                }
            }
        }

        if !changedFiles.isEmpty {
            let count = changedFiles.count
            let shown = Array(changedFiles.prefix(2))
            let filePart: String
            if shown.count == 1 {
                filePart = shown[0]
            } else {
                filePart = shown.joined(separator: ", ")
            }

            let morePart = count > shown.count ? " · +\(count - shown.count) more" : ""
            return "\(statusLine) · \(count) file\(count == 1 ? "" : "s") · \(filePart)\(morePart)"
        }

        var lines: [String] = [statusLine]
        if let stdout,
           let first = stdout
            .components(separatedBy: "\n")
            .map({ $0.trimmingCharacters(in: .whitespaces) })
            .first(where: { !$0.isEmpty }) {
            let candidate = looksLikePathLine(first) ? compactDisplayPath(first) : first
            lines.append(truncate(candidate, maxCharacters: 220))
        }

        return lines.joined(separator: "\n")
    }

    private func summarizeReplayHistory(_ object: [String: JSONValue]) -> String {
        guard let replayItems = object["items"]?.arrayValue,
              !replayItems.isEmpty
        else {
            return "Restored history"
        }

        var samples: [String] = []
        samples.reserveCapacity(2)

        for item in replayItems {
            guard let messageObject = item.objectValue else {
                continue
            }

            let roleType = replayMessageRole(from: messageObject)
            let role = replayRoleDisplayLabel(roleType)
            let text = replayMessageTextValue(from: messageObject)
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else {
                continue
            }

            if shouldSkipReplayMessage(role: roleType, text: trimmed) {
                continue
            }

            samples.append("\(role): \(truncate(trimmed, maxCharacters: 140))")
            if samples.count == 2 {
                break
            }
        }

        if samples.isEmpty {
            return "Restored history · \(replayItems.count) messages"
        }

        let heading = "Restored history · \(replayItems.count) messages"
        return "\(heading)\n\(samples.joined(separator: "\n"))"
    }

    private func replayRoleLabel(from object: [String: JSONValue]) -> String {
        replayRoleDisplayLabel(replayMessageRole(from: object))
    }

    private func replayRoleDisplayLabel(_ role: ReplayHistoryMessage.Role) -> String {
        switch role {
        case .assistant:
            return "Assistant"
        case .user:
            return "You"
        case .unknown:
            return "Message"
        }
    }

    private func replayMessageTextValue(from object: [String: JSONValue]) -> String {
        if let message = object["message"]?.stringValue {
            return normalizeStructuredText(message)
        }

        if let content = object["content"]?.arrayValue {
            let chunks = content.compactMap(replayTextChunkValue(from:)).filter { !$0.isEmpty }
            if !chunks.isEmpty {
                return chunks.joined(separator: "\n")
            }
        }

        if let text = object["text"]?.stringValue {
            return normalizeStructuredText(text)
        }

        return ""
    }

    private func replayTextChunkValue(from value: JSONValue) -> String? {
        guard let object = value.objectValue else {
            return nil
        }

        if let text = object["text"]?.stringValue {
            return normalizeStructuredText(text)
        }

        if let nested = object["content"]?.stringValue {
            return normalizeStructuredText(nested)
        }

        return nil
    }

    private func formatTokenCount(_ value: Int) -> String {
        Self.tokenNumberFormatter.string(from: NSNumber(value: value)) ?? "\(value)"
    }

    private func joinedCommand(from value: JSONValue?) -> String {
        guard let array = value?.arrayValue else {
            return ""
        }

        return array.compactMap(\.stringValue).joined(separator: " ")
    }

    private func formatCompactTokenCount(_ value: Int) -> String {
        if value >= 1_000_000 {
            let millions = Double(value) / 1_000_000
            let compact = Self.compactTokenNumberFormatter.string(from: NSNumber(value: millions)) ?? "\(millions)"
            return "\(compact)M"
        }

        if value >= 1_000 {
            let thousands = Double(value) / 1_000
            let compact = Self.compactTokenNumberFormatter.string(from: NSNumber(value: thousands)) ?? "\(thousands)"
            return "\(compact)k"
        }

        return formatTokenCount(value)
    }

    private func compactDisplayPath(_ value: String) -> String {
        var normalized = value.replacingOccurrences(of: "\\", with: "/")
        while normalized.hasPrefix("../") {
            normalized.removeFirst(3)
        }
        while normalized.hasPrefix("./") {
            normalized.removeFirst(2)
        }

        let components = normalized.split(separator: "/")
        if components.isEmpty {
            return normalized
        }

        if components.count == 1 {
            return normalized
        }

        return String(components.last ?? Substring(normalized))
    }

    private func looksLikePathLine(_ value: String) -> Bool {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return false
        }

        let normalized = trimmed.replacingOccurrences(of: "\\", with: "/")
        guard normalized.contains("/") else {
            return false
        }

        let lowercase = normalized.lowercased()
        let fileLikeSuffixes = [
            ".swift", ".rs", ".ts", ".tsx", ".js", ".jsx", ".json", ".toml", ".md", ".yml", ".yaml", ".py", ".go", ".java", ".kt", ".mm", ".m", ".cpp", ".c", ".h", ".hpp"
        ]

        if fileLikeSuffixes.contains(where: { lowercase.hasSuffix($0) }) {
            return true
        }

        return lowercase.contains("/sources/") || lowercase.contains("/src/")
    }

    private func normalizeStructuredText(_ value: String) -> String {
        var normalized = value
        for _ in 0..<3 {
            let next = normalized
                .replacingOccurrences(of: "\\n", with: "\n")
                .replacingOccurrences(of: "\\t", with: "\t")
                .replacingOccurrences(of: "\\\"", with: "\"")
            if next == normalized {
                break
            }
            normalized = next
        }
        return normalized
    }

    private func normalizeReasoningText(_ value: String) -> String {
        normalizeStructuredText(value)
            .replacingOccurrences(of: "**", with: "")
            .replacingOccurrences(of: "__", with: "")
            .replacingOccurrences(of: "`", with: "")
    }

    private func truncate(_ value: String, maxCharacters: Int) -> String {
        guard let end = value.index(value.startIndex, offsetBy: maxCharacters, limitedBy: value.endIndex) else {
            return value
        }

        guard end != value.endIndex else {
            return value
        }

        return "\(value[..<end])\n…"
    }

    private static let gitDiffHeaderBytes = Array("diff --git ".utf8)

    private func countGitDiffHeaders(in value: String) -> Int {
        let header = SessionStreamItem.gitDiffHeaderBytes
        var count = 0
        var headerIndex = 0
        var atLineStart = true

        for byte in value.utf8 {
            if atLineStart {
                if byte == header[headerIndex] {
                    headerIndex += 1
                    if headerIndex == header.count {
                        count += 1
                        atLineStart = false
                        headerIndex = 0
                    }
                    continue
                }

                atLineStart = false
                headerIndex = 0
            }

            if byte == 0x0A || byte == 0x0D {
                atLineStart = true
                headerIndex = 0
            }
        }

        return count
    }
}

struct CoreEventPayload: Decodable, Hashable {
    let id: String
    let eventSeq: UInt64
    let kind: String
    let payload: JSONValue?
}

enum ServerEnvelope: Decodable {
    case hello(HelloMessage)
    case sessionList(SessionListMessage)
    case sessionCreated(SessionCreatedMessage)
    case sessionAttached(SessionAttachedMessage)
    case sessionDetached(SessionDetachedMessage)
    case sessionStream(SessionStreamMessage)
    case error(ErrorMessage)
    case ack
    case unknown(String)

    private enum CodingKeys: String, CodingKey {
        case type
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)

        switch type {
        case "hello":
            self = .hello(try HelloMessage(from: decoder))
        case "session_list":
            self = .sessionList(try SessionListMessage(from: decoder))
        case "session_created":
            self = .sessionCreated(try SessionCreatedMessage(from: decoder))
        case "session_attached":
            self = .sessionAttached(try SessionAttachedMessage(from: decoder))
        case "session_detached":
            self = .sessionDetached(try SessionDetachedMessage(from: decoder))
        case "session_stream":
            self = .sessionStream(try SessionStreamMessage(from: decoder))
        case "error":
            self = .error(try ErrorMessage(from: decoder))
        case "ack":
            self = .ack
        default:
            self = .unknown(type)
        }
    }
}

struct OutboundMessage: Encodable {
    let type: String
    let requestId: String?
    let sessionId: UUID?
    let fromSeq: UInt64?

    static func listSessions(requestId: String) -> Self {
        Self(type: "list_sessions", requestId: requestId, sessionId: nil, fromSeq: nil)
    }

    static func attachSession(requestId: String, sessionId: UUID, fromSeq: UInt64) -> Self {
        Self(type: "attach_session", requestId: requestId, sessionId: sessionId, fromSeq: fromSeq)
    }

    static func detachSession(requestId: String, sessionId: UUID) -> Self {
        Self(type: "detach_session", requestId: requestId, sessionId: sessionId, fromSeq: nil)
    }
}

struct CreateSessionMessage: Encodable {
    let type: String = "create_session"
    let requestId: String
    let cwd: String?
}

struct ComposerUpdateMessage: Encodable {
    let type: String = "composer_update"
    let requestId: String
    let sessionId: UUID
    let text: String
    let cursor: Int
}

struct SubmitTurnMessage: Encodable {
    let type: String = "submit_turn"
    let requestId: String
    let sessionId: UUID
}

struct InterruptTurnMessage: Encodable {
    let type: String = "interrupt_turn"
    let requestId: String
    let sessionId: UUID
}

struct ExecApprovalMessage: Encodable {
    let type: String = "exec_approval"
    let requestId: String
    let sessionId: UUID
    let callId: String
    let decision: String
}

struct PatchApprovalMessage: Encodable {
    let type: String = "patch_approval"
    let requestId: String
    let sessionId: UUID
    let callId: String
    let decision: String
}

enum ApprovalType {
    case exec
    case patch
}

enum ApprovalDecisionChoice: String, CaseIterable, Identifiable {
    case approved
    case approvedForSession
    case denied

    var id: String { rawValue }

    var wireValue: String {
        switch self {
        case .approved:
            return "approved"
        case .approvedForSession:
            return "approved_for_session"
        case .denied:
            return "denied"
        }
    }

    var label: String {
        switch self {
        case .approved:
            return "Yes"
        case .approvedForSession:
            return "Yes, and don't ask again for this session"
        case .denied:
            return "No, deny this request"
        }
    }
}

struct ApprovalRequest {
    let type: ApprovalType
    let callId: String
}

struct ExecCommandInfo {
    let command: String
    let output: String
    let exitCode: Int?
    let duration: String?
}

struct ReplayHistoryMessage: Hashable, Identifiable {
    enum Role: Hashable {
        case user
        case assistant
        case unknown
    }

    let id: String
    let role: Role
    let text: String
}

enum JSONValue: Decodable, Hashable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case object([String: JSONValue])
    case array([JSONValue])
    case null

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()

        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([String: JSONValue].self) {
            self = .object(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else {
            throw DecodingError.dataCorruptedError(
                in: container,
                debugDescription: "Unsupported JSON value"
            )
        }
    }

    var typeHint: String? {
        if case .object(let object) = self,
           case .string(let value) = object["type"] {
            return value
        }
        return nil
    }

    var pretty: String {
        switch self {
        case .string(let value):
            return value
        case .number(let value):
            return String(value)
        case .bool(let value):
            return String(value)
        case .null:
            return "null"
        case .array, .object:
            guard
                let object = asJSONObject,
                JSONSerialization.isValidJSONObject(object),
                let data = try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted]),
                let value = String(data: data, encoding: .utf8)
            else {
                return "(unprintable JSON)"
            }
            return value
        }
    }

    private var asJSONObject: Any? {
        switch self {
        case .string(let value):
            return value
        case .number(let value):
            return value
        case .bool(let value):
            return value
        case .null:
            return NSNull()
        case .array(let values):
            return values.compactMap { $0.asJSONObject }
        case .object(let values):
            return values.reduce(into: [String: Any]()) { partialResult, entry in
                partialResult[entry.key] = entry.value.asJSONObject
            }
        }
    }
}

extension JSONValue {
    var objectValue: [String: JSONValue]? {
        if case .object(let value) = self {
            return value
        }
        return nil
    }

    var stringValue: String? {
        if case .string(let value) = self {
            return value
        }
        return nil
    }

    var numberValue: Double? {
        if case .number(let value) = self {
            return value
        }
        return nil
    }

    var boolValue: Bool? {
        if case .bool(let value) = self {
            return value
        }
        return nil
    }

    var arrayValue: [JSONValue]? {
        if case .array(let value) = self {
            return value
        }
        return nil
    }
}

extension SessionStreamItem {
    private var isTransientTransportErrorLine: Bool {
        let candidates = [
            message,
            event?.payload?.pretty,
            body,
        ]
        .compactMap { $0?.lowercased() }

        for text in candidates {
            if text.contains("stream disconnected before completion") {
                return true
            }
            if text.contains("transport error") && text.contains("retrying in") {
                return true
            }
            if text.contains("error decoding response body") && text.contains("retrying") {
                return true
            }
        }

        return false
    }

    var isTokenCountEvent: Bool {
        guard type == "core_event" else {
            return false
        }
        return event?.payload?.typeHint == "token_count"
    }

    var isPatchApplyEndEvent: Bool {
        guard type == "core_event" else {
            return false
        }
        return event?.payload?.typeHint == "patch_apply_end"
    }

    var isBackgroundEvent: Bool {
        guard type == "core_event" else {
            return false
        }
        return event?.payload?.typeHint == "background_event"
    }

    var isTurnAbortedEvent: Bool {
        guard type == "core_event" else {
            return false
        }
        return event?.payload?.typeHint == "turn_aborted"
    }

    var isReplayHistoryEvent: Bool {
        guard type == "core_event" else {
            return false
        }
        return event?.payload?.typeHint == "replay_history"
    }

    var tokenCountRequestedModel: String? {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "token_count",
              let object = payload.objectValue,
              let info = object["info"]?.objectValue,
              let model = info["requested_model"]?.stringValue,
              !model.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else {
            return nil
        }

        return model
    }

    var assistantMessageText: String? {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "agent_message",
              let object = payload.objectValue,
              let message = object["message"]?.stringValue
        else {
            return nil
        }

        if isInternalAssistantStatusMessage(message) {
            return nil
        }

        return message
    }

    var replayHistoryMessages: [ReplayHistoryMessage] {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "replay_history",
              let object = payload.objectValue,
              let replayItems = object["items"]?.arrayValue,
              !replayItems.isEmpty
        else {
            return []
        }

        var parsed: [ReplayHistoryMessage] = []
        parsed.reserveCapacity(replayItems.count)

        for (index, item) in replayItems.enumerated() {
            guard let itemObject = item.objectValue else {
                continue
            }

            let role = replayMessageRole(from: itemObject)

            let text = replayMessageText(from: itemObject)
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else {
                continue
            }

            if shouldSkipReplayMessage(role: role, text: trimmed) {
                continue
            }

            let message = ReplayHistoryMessage(id: "\(seq)-\(index)", role: role, text: trimmed)

            if role == .assistant,
               let last = parsed.last,
               last.role == .assistant {
                parsed[parsed.count - 1] = message
                continue
            }

            parsed.append(message)
        }

        return parsed
    }

    private func replayMessageRole(from object: [String: JSONValue]) -> ReplayHistoryMessage.Role {
        let roleValue = object["role"]?.stringValue?.lowercased() ?? ""
        switch roleValue {
        case "assistant":
            return .assistant
        case "user":
            return .user
        default:
            let payloadType = object["type"]?.stringValue?.lowercased() ?? ""
            if payloadType == "agent_message" {
                return .assistant
            }
            if payloadType == "user_message" {
                return .user
            }
            return .unknown
        }
    }

    private func replayMessageText(from object: [String: JSONValue]) -> String {
        if let message = object["message"]?.stringValue {
            return normalizeStructuredText(message)
        }

        if let content = object["content"]?.arrayValue {
            let chunks = content.compactMap(replayTextChunk(from:)).filter { !$0.isEmpty }
            if !chunks.isEmpty {
                return chunks.joined(separator: "\n")
            }
        }

        if let text = object["text"]?.stringValue {
            return normalizeStructuredText(text)
        }

        if let summary = object["summary"]?.stringValue {
            return normalizeStructuredText(summary)
        }

        return ""
    }

    private func replayTextChunk(from value: JSONValue) -> String? {
        guard let object = value.objectValue else {
            return nil
        }

        if let text = object["text"]?.stringValue {
            return normalizeStructuredText(text)
        }

        if let nested = object["content"]?.stringValue {
            return normalizeStructuredText(nested)
        }

        return nil
    }

    private func shouldSkipReplayMessage(role: ReplayHistoryMessage.Role, text: String) -> Bool {
        if role == .user,
           isImagePlaceholderMessage(text) {
            return true
        }

        if role == .assistant {
            if isInternalAssistantStatusMessage(text) {
                return true
            }
        }

        return false
    }

    var userMessageText: String? {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "user_message",
              let object = payload.objectValue,
              let message = object["message"]?.stringValue
        else {
            return nil
        }
        return message
    }

    var isImagePlaceholderUserMessage: Bool {
        guard let userMessageText else {
            return false
        }

        let trimmed = userMessageText.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.hasPrefix("[image:") && trimmed.hasSuffix("]")
    }

    var threadNameUpdate: String? {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "thread_name_updated",
              let object = payload.objectValue,
              let name = object["thread_name"]?.stringValue
        else {
            return nil
        }

        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    var execCommandInfo: ExecCommandInfo? {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "exec_command_end",
              let object = payload.objectValue
        else {
            return nil
        }

        let command = object["command"]?.arrayValue?
            .compactMap(\.stringValue)
            .joined(separator: " ") ?? ""

        let exitCode = object["exit_code"]?.numberValue.map(Int.init)
        let duration = object["duration"]?.stringValue

        let output = object["formatted_output"]?.stringValue
            ?? object["aggregated_output"]?.stringValue
            ?? object["stdout"]?.stringValue
            ?? object["stderr"]?.stringValue
            ?? ""

        return ExecCommandInfo(
            command: command,
            output: normalizeStructuredText(output),
            exitCode: exitCode,
            duration: duration
        )
    }

    var isConversationBoundaryEvent: Bool {
        guard type == "core_event" else {
            return false
        }

        let payloadType = event?.payload?.typeHint ?? ""
        return payloadType == "user_message" || payloadType == "agent_message"
    }

    var approvalRequest: ApprovalRequest? {
        guard type == "core_event",
              let payload = event?.payload,
              let object = payload.objectValue,
              let payloadType = object["type"]?.stringValue,
              let callId = object["call_id"]?.stringValue
        else {
            return nil
        }

        switch payloadType {
        case "exec_approval_request":
            return ApprovalRequest(type: .exec, callId: callId)
        case "apply_patch_approval_request":
            return ApprovalRequest(type: .patch, callId: callId)
        default:
            return nil
        }
    }

    var turnDiffText: String? {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "turn_diff",
              let object = payload.objectValue,
              let unifiedDiff = object["unified_diff"]?.stringValue
        else {
            return nil
        }

        return normalizeStructuredText(unifiedDiff)
    }

    var isReplayOmittedNotice: Bool {
        guard type == "system",
              let message,
              message.contains("Large historical item omitted from replay")
        else {
            return false
        }
        return true
    }

    var shouldHideFromTranscript: Bool {
        if isTransientTransportErrorLine {
            return true
        }

        if type == "system",
           let message {
            let normalized = message.lowercased()
            if normalized.contains("stream disconnected before completion") {
                return true
            }

            if normalized.contains("transport error") && normalized.contains("retrying in") {
                return true
            }

            return false
        }

        guard type == "core_event",
              let payloadType = event?.payload?.typeHint
        else {
            return false
        }

        if payloadType == "session_attached" || payloadType == "session_detached" || payloadType == "thread_name_updated" {
            return true
        }

        if payloadType == "agent_reasoning_section_break" || payloadType == "agent_reasoning" {
            return true
        }

        if payloadType == "task_started" {
            return true
        }

        if payloadType == "task_complete" {
            return true
        }

        if payloadType == "plan_update" {
            return true
        }

        if payloadType == "exec_command_begin" || payloadType == "exec_command_output_delta" {
            return true
        }

        if payloadType == "agent_message",
           let payload = event?.payload,
           let object = payload.objectValue,
           let message = object["message"]?.stringValue,
           isInternalAssistantStatusMessage(message) {
            return true
        }

        // Internal image inspection tool calls add transcript noise and can leak
        // long local file paths without adding user-facing context.
        if payloadType.contains("view_image_tool_call") {
            return true
        }

        if payloadType == "user_message",
           let payload = event?.payload,
           let object = payload.objectValue,
           let message = object["message"]?.stringValue,
           isImagePlaceholderMessage(message) {
            return true
        }

        return false
    }

    private func isImagePlaceholderMessage(_ message: String) -> Bool {
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.hasPrefix("[image:") && trimmed.hasSuffix("]")
    }

    private func isInternalAssistantStatusMessage(_ message: String) -> Bool {
        let normalized = message
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()

        return normalized.hasPrefix("background shell completed (")
            || normalized.contains("sandbox error: command was killed by a signal")
            || normalized.hasPrefix("[developer] background auto-review completed")
            || normalized.hasPrefix("background auto-review completed")
    }

    var cardStyle: TranscriptCardStyle {
        switch type {
        case "system":
            return .system
        case "composer":
            return .composer
        case "core_event":
            if approvalRequest != nil {
                return .approval
            }

            let payloadType = event?.payload?.typeHint ?? ""
            if payloadType == "user_message" {
                return .user
            }
            if payloadType == "agent_message" {
                return .assistant
            }
            if payloadType.contains("reasoning") {
                return .reasoning
            }
            if payloadType == "turn_diff" {
                return .tool
            }
            if payloadType.contains("exec") || payloadType.contains("tool") {
                return .tool
            }
            return .system
        default:
            return .defaultStyle
        }
    }

    var prefersTrailingBubble: Bool {
        switch cardStyle {
        case .user, .composer:
            return true
        case .defaultStyle, .assistant, .reasoning, .tool, .approval, .system:
            return false
        }
    }

    var prefersCenteredBubble: Bool {
        isPatchApplyEndEvent || isTokenCountEvent || isBackgroundEvent
    }
}

enum TranscriptCardStyle {
    case defaultStyle
    case user
    case assistant
    case reasoning
    case tool
    case approval
    case composer
    case system
}
