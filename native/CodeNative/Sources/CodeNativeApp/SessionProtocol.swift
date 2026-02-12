import Foundation

struct SessionSummary: Decodable, Hashable, Identifiable {
    let id: UUID
    let conversationID: String
    let model: String
    let cwd: String
    let createdAtUnixMs: UInt64
}

struct SessionAttachedMessage: Decodable {
    let sessionID: UUID
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
    let sessionID: UUID
}

struct SessionListMessage: Decodable {
    let sessions: [SessionSummary]
}

struct HelloMessage: Decodable {
    let clientID: String
}

struct ErrorMessage: Decodable {
    let requestID: String?
    let message: String
}

struct SessionStreamItem: Decodable, Hashable, Identifiable {
    let type: String
    let sessionID: UUID
    let seq: UInt64
    let event: CoreEventPayload?
    let rev: UInt64?
    let text: String?
    let cursor: Int?
    let sourceClientID: String?
    let level: String?
    let message: String?

    var id: String {
        "\(sessionID.uuidString)-\(seq)-\(type)"
    }

    var title: String {
        switch type {
        case "core_event":
            return event?.payload?.typeHint ?? event?.kind ?? type
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
            return event?.payload?.pretty ?? "(empty core payload)"
        case "composer":
            return text ?? ""
        case "system":
            return message ?? ""
        default:
            return message ?? ""
        }
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
    let requestID: String?
    let sessionID: UUID?
    let fromSeq: UInt64?

    static func listSessions(requestID: String) -> Self {
        Self(type: "list_sessions", requestID: requestID, sessionID: nil, fromSeq: nil)
    }

    static func attachSession(requestID: String, sessionID: UUID, fromSeq: UInt64) -> Self {
        Self(type: "attach_session", requestID: requestID, sessionID: sessionID, fromSeq: fromSeq)
    }

    static func detachSession(requestID: String, sessionID: UUID) -> Self {
        Self(type: "detach_session", requestID: requestID, sessionID: sessionID, fromSeq: nil)
    }
}

struct ComposerUpdateMessage: Encodable {
    let type: String = "composer_update"
    let requestID: String
    let sessionID: UUID
    let text: String
    let cursor: Int
}

struct SubmitTurnMessage: Encodable {
    let type: String = "submit_turn"
    let requestID: String
    let sessionID: UUID
}

struct InterruptTurnMessage: Encodable {
    let type: String = "interrupt_turn"
    let requestID: String
    let sessionID: UUID
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
}

extension SessionStreamItem {
    var assistantMessageText: String? {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "agent_message",
              let object = payload.objectValue,
              let message = object["message"]?.stringValue
        else {
            return nil
        }
        return message
    }
}
