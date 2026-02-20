import Foundation

enum CompanionConnectionState: Equatable {
    case discovering
    case pairRequired
    case approvalPending
    case connected
    case reconnecting
    case offline

    var label: String {
        switch self {
        case .discovering:
            return "Discovering"
        case .pairRequired:
            return "Pair required"
        case .approvalPending:
            return "Approval pending"
        case .connected:
            return "Connected"
        case .reconnecting:
            return "Reconnecting"
        case .offline:
            return "Offline"
        }
    }

    var detail: String {
        switch self {
        case .discovering:
            return "Searching for your Mac companion endpoint and validating pairing token."
        case .pairRequired:
            return "Import a fresh pairing code or rotate your companion token."
        case .approvalPending:
            return "A companion approval is waiting before work can continue."
        case .connected:
            return "Companion tunnel is healthy and ready for session attach."
        case .reconnecting:
            return "Trying to restore the companion session after a disconnect."
        case .offline:
            return "Companion is unreachable. Use LAN/manual endpoint fallback and reconnect."
        }
    }

    static func resolve(
        connectionState: SessionMirrorStore.ConnectionState,
        statusLine: String,
        lastError: String?,
        hasPendingApproval: Bool
    ) -> CompanionConnectionState {
        if hasPendingApproval {
            return .approvalPending
        }

        let normalized = normalizedStatusText(statusLine: statusLine, lastError: lastError)

        if normalized.contains("pair required")
            || normalized.contains("unauthorized")
            || normalized.contains("401")
        {
            return .pairRequired
        }

        switch connectionState {
        case .connected:
            return .connected
        case .connecting:
            if normalized.contains("reconnect") {
                return .reconnecting
            }
            return .discovering
        case .disconnected:
            if normalized.contains("reconnect") {
                return .reconnecting
            }
            return .offline
        }
    }

    private static func normalizedStatusText(statusLine: String, lastError: String?) -> String {
        let status = statusLine.trimmingCharacters(in: .whitespacesAndNewlines)
        let error = lastError?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let combined = "\(status) \(error)"
        return combined.lowercased()
    }
}
