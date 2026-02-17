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

    init(
        id: UUID,
        conversationId: String,
        model: String,
        cwd: String,
        createdAtUnixMs: UInt64,
        lastEventAtUnixMs: UInt64,
        title: String?
    ) {
        self.id = id
        self.conversationId = conversationId
        self.model = model
        self.cwd = cwd
        self.createdAtUnixMs = createdAtUnixMs
        self.lastEventAtUnixMs = lastEventAtUnixMs
        self.title = title
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
    let hasMoreBefore: Bool?
    let items: [SessionStreamItem]
}

struct SessionHistoryPageMessage: Decodable {
    let requestId: String?
    let sessionId: UUID
    let beforeSeq: UInt64
    let hasMoreBefore: Bool
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
            case "collab_agent_spawn_begin", "collab_agent_spawn_end", "collab_agent_interaction_begin", "collab_agent_interaction_end", "collab_waiting_begin", "collab_waiting_end", "collab_close_begin", "collab_close_end", "collab_resume_begin", "collab_resume_end":
                return "coordinator"
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
            return summarizeSystemMessage(message)
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

        if let collaborationProgress = collaborationProgressEvent {
            return summarizeCollaborationProgress(collaborationProgress)
        }

        if let browserWorkflow = browserWorkflowEvent {
            return summarizeBrowserWorkflow(browserWorkflow)
        }

        if payloadType == "agent_message",
           let message = object["message"]?.stringValue {
            if isAutoReviewSummaryMessage(message) || isAutoReviewStreamSummaryMessage(message) {
                return summarizeAutoReviewMessage(message)
            }
            return normalizeStructuredText(message)
        }

        if payloadType == "user_message",
           let message = object["message"]?.stringValue {
            let sanitizedMessage = sanitizeUserMessage(message)
            if sanitizedMessage.isEmpty {
                return ""
            }
            if isImagePlaceholderMessage(sanitizedMessage) {
                return "Shared image"
            }
            return normalizeStructuredText(sanitizedMessage)
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

        if payloadType == "request_user_input" {
            return summarizeRequestUserInput(object)
        }

        if payloadType == "user_input_answer" {
            return summarizeUserInputAnswer(object)
        }

        if payloadType == "task_started" {
            return summarizeTaskLifecycle(object: object, phase: "started")
        }

        if payloadType == "task_complete" {
            return summarizeTaskLifecycle(object: object, phase: "complete")
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

        let hasDirectionalBreakdown = (input ?? 0) > 0 || (output ?? 0) > 0 || (reasoning ?? 0) > 0

        if !hasDirectionalBreakdown,
           let total,
           total > 0 {
            parts.append("\(formatCompactTokenCount(total)) tok")
        }

        if let input,
           input > 0 {
            parts.append("in \(formatCompactTokenCount(input))")
        }
        if let output,
           output > 0 {
            parts.append("out \(formatCompactTokenCount(output))")
        }
        if let reasoning,
           reasoning > 0 {
            parts.append("reason \(formatCompactTokenCount(reasoning))")
        }

        if parts.count == 1 {
            parts.append("usage updated")
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

    private func summarizeTaskLifecycle(object: [String: JSONValue], phase: String) -> String {
        let headline = "Task \(phase)"

        let candidates: [String?] = [
            object["message"]?.stringValue,
            object["title"]?.stringValue,
            object["task"]?.stringValue,
            object["summary"]?.stringValue,
            object["detail"]?.stringValue,
            object["payload"]?.objectValue?["message"]?.stringValue,
            object["payload"]?.objectValue?["title"]?.stringValue,
        ]

        for candidate in candidates {
            guard let candidate else {
                continue
            }

            let normalized = normalizeStructuredText(candidate).trimmingCharacters(in: .whitespacesAndNewlines)
            if !normalized.isEmpty {
                return "\(headline)\n\(normalized)"
            }
        }

        return headline
    }

    private func summarizeCollaborationProgress(_ event: CollaborationProgressEvent) -> String {
        var lines: [String] = [event.headline]
        if !event.detailLines.isEmpty {
            lines.append(contentsOf: event.detailLines.prefix(5))
        }

        if let artifactPreview = event.artifactPreview,
           !artifactPreview.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            lines.append(artifactPreview)
        }

        return lines.joined(separator: "\n")
    }

    private func parseCollaborationProgressEvent(
        payloadType: String,
        object: [String: JSONValue]
    ) -> CollaborationProgressEvent? {
        switch payloadType {
        case "collab_agent_spawn_begin":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let senderThread = normalizedBrowserValue(object["sender_thread_id"]?.stringValue)
            let prompt = normalizedBrowserValue(object["prompt"]?.stringValue)
            var details: [String] = []
            if let senderThread {
                details.append("Coordinator: \(shortThreadID(senderThread))")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }
            if let prompt,
               !prompt.isEmpty {
                details.append("Prompt: \(truncate(prompt, maxCharacters: 180))")
            }

            return CollaborationProgressEvent(
                status: .inProgress,
                headline: "Spawning helper agent",
                detailLines: details,
                artifactPreview: nil,
                callId: callId
            )

        case "collab_agent_spawn_end":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let senderThread = normalizedBrowserValue(object["sender_thread_id"]?.stringValue)
            let agentThread = normalizedBrowserValue(object["new_thread_id"]?.stringValue)
            let statusSummary = parseCollaborationAgentStatus(object["status"])
            var details: [String] = []
            if let senderThread {
                details.append("Coordinator: \(shortThreadID(senderThread))")
            }
            if let agentThread {
                details.append("Agent: \(shortThreadID(agentThread))")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }
            details.append("Status: \(statusSummary.label)")

            return CollaborationProgressEvent(
                status: statusSummary.status,
                headline: statusSummary.status == .failed ? "Helper agent spawn failed" : "Helper agent ready",
                detailLines: details,
                artifactPreview: statusSummary.detail,
                callId: callId
            )

        case "collab_agent_interaction_begin":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let senderThread = normalizedBrowserValue(object["sender_thread_id"]?.stringValue)
            let receiverThread = normalizedBrowserValue(object["receiver_thread_id"]?.stringValue)
            let prompt = normalizedBrowserValue(object["prompt"]?.stringValue)

            var details: [String] = []
            if let senderThread,
               let receiverThread {
                details.append("\(shortThreadID(senderThread)) → \(shortThreadID(receiverThread))")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }
            if let prompt,
               !prompt.isEmpty {
                details.append("Prompt: \(truncate(prompt, maxCharacters: 180))")
            }

            return CollaborationProgressEvent(
                status: .inProgress,
                headline: "Delegating work to helper agent",
                detailLines: details,
                artifactPreview: nil,
                callId: callId
            )

        case "collab_agent_interaction_end":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let senderThread = normalizedBrowserValue(object["sender_thread_id"]?.stringValue)
            let receiverThread = normalizedBrowserValue(object["receiver_thread_id"]?.stringValue)
            let statusSummary = parseCollaborationAgentStatus(object["status"])

            var details: [String] = []
            if let senderThread,
               let receiverThread {
                details.append("\(shortThreadID(senderThread)) ← \(shortThreadID(receiverThread))")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }
            details.append("Status: \(statusSummary.label)")

            return CollaborationProgressEvent(
                status: statusSummary.status,
                headline: statusSummary.status == .failed ? "Helper agent response failed" : "Helper agent response received",
                detailLines: details,
                artifactPreview: statusSummary.detail,
                callId: callId
            )

        case "collab_waiting_begin":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let senderThread = normalizedBrowserValue(object["sender_thread_id"]?.stringValue)
            let receiverThreadIDs = object["receiver_thread_ids"]?.arrayValue?
                .compactMap(\.stringValue)
                .map(shortThreadID) ?? []

            var details: [String] = []
            if let senderThread {
                details.append("Coordinator: \(shortThreadID(senderThread))")
            }
            if !receiverThreadIDs.isEmpty {
                details.append("Agents: \(receiverThreadIDs.joined(separator: " · "))")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }

            let headline = receiverThreadIDs.isEmpty
                ? "Waiting for helper agents"
                : "Waiting for \(receiverThreadIDs.count) helper agent\(receiverThreadIDs.count == 1 ? "" : "s")"

            return CollaborationProgressEvent(
                status: .inProgress,
                headline: headline,
                detailLines: details,
                artifactPreview: nil,
                callId: callId
            )

        case "collab_waiting_end":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let senderThread = normalizedBrowserValue(object["sender_thread_id"]?.stringValue)
            let statusByThread = object["statuses"]?.objectValue ?? [:]

            var completed = 0
            var running = 0
            var failed = 0
            var threadLines: [String] = []
            var artifactLines: [String] = []

            for (threadID, statusValue) in statusByThread {
                let status = parseCollaborationAgentStatus(statusValue)
                switch status.status {
                case .succeeded:
                    completed += 1
                case .inProgress:
                    running += 1
                case .failed:
                    failed += 1
                }
                threadLines.append("\(shortThreadID(threadID)): \(status.label)")
                if let detail = status.detail,
                   !detail.isEmpty,
                   artifactLines.count < 2 {
                    artifactLines.append("\(shortThreadID(threadID)): \(detail)")
                }
            }

            threadLines.sort()

            let eventStatus: CollaborationProgressStatus
            let headline: String
            if failed > 0 {
                eventStatus = .failed
                headline = "Agent wait ended with errors"
            } else if running > 0 {
                eventStatus = .inProgress
                headline = "Agent wait still in progress"
            } else {
                eventStatus = .succeeded
                headline = "All helper agents reported"
            }

            var details: [String] = []
            if let senderThread {
                details.append("Coordinator: \(shortThreadID(senderThread))")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }
            if !statusByThread.isEmpty {
                details.append("Completed: \(completed) · Running: \(running) · Errors: \(failed)")
                details.append(contentsOf: threadLines.prefix(3))
            }

            let artifactPreview = artifactLines.isEmpty ? nil : artifactLines.joined(separator: "\n")
            return CollaborationProgressEvent(
                status: eventStatus,
                headline: headline,
                detailLines: details,
                artifactPreview: artifactPreview,
                callId: callId
            )

        case "collab_close_begin", "collab_resume_begin":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let senderThread = normalizedBrowserValue(object["sender_thread_id"]?.stringValue)
            let receiverThread = normalizedBrowserValue(object["receiver_thread_id"]?.stringValue)
            var details: [String] = []
            if let senderThread,
               let receiverThread {
                details.append("\(shortThreadID(senderThread)) ↔ \(shortThreadID(receiverThread))")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }

            return CollaborationProgressEvent(
                status: .inProgress,
                headline: payloadType == "collab_close_begin"
                    ? "Closing helper agent session"
                    : "Resuming helper agent session",
                detailLines: details,
                artifactPreview: nil,
                callId: callId
            )

        case "collab_close_end", "collab_resume_end":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let senderThread = normalizedBrowserValue(object["sender_thread_id"]?.stringValue)
            let receiverThread = normalizedBrowserValue(object["receiver_thread_id"]?.stringValue)
            let statusSummary = parseCollaborationAgentStatus(object["status"])
            var details: [String] = []
            if let senderThread,
               let receiverThread {
                details.append("\(shortThreadID(senderThread)) ↔ \(shortThreadID(receiverThread))")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }
            details.append("Status: \(statusSummary.label)")

            return CollaborationProgressEvent(
                status: statusSummary.status,
                headline: payloadType == "collab_close_end"
                    ? (statusSummary.status == .failed ? "Helper close failed" : "Helper session closed")
                    : (statusSummary.status == .failed ? "Helper resume failed" : "Helper session resumed"),
                detailLines: details,
                artifactPreview: statusSummary.detail,
                callId: callId
            )

        default:
            return nil
        }
    }

    private func parseCollaborationAgentStatus(_ value: JSONValue?) -> CollaborationAgentStatusSummary {
        guard let value else {
            return CollaborationAgentStatusSummary(status: .inProgress, label: "running", detail: nil)
        }

        if let status = value.stringValue {
            return collaborationStatusSummary(from: status, detail: nil)
        }

        if let object = value.objectValue {
            if let completed = object["completed"] {
                let detail = normalizedBrowserValue(completed.stringValue) ?? normalizedBrowserValue(completed.pretty)
                return collaborationStatusSummary(from: "completed", detail: detail)
            }

            if let errored = object["errored"] {
                let detail = normalizedBrowserValue(errored.stringValue) ?? normalizedBrowserValue(errored.pretty)
                return collaborationStatusSummary(from: "errored", detail: detail)
            }

            let knownStatuses = ["pending_init", "running", "shutdown", "not_found"]
            for known in knownStatuses where object[known] != nil {
                return collaborationStatusSummary(from: known, detail: nil)
            }

            let preview = truncate(value.pretty, maxCharacters: 220)
            return CollaborationAgentStatusSummary(status: .inProgress, label: "running", detail: preview)
        }

        return CollaborationAgentStatusSummary(status: .inProgress, label: "running", detail: nil)
    }

    private func collaborationStatusSummary(from rawValue: String, detail: String?) -> CollaborationAgentStatusSummary {
        let normalized = rawValue.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        switch normalized {
        case "completed", "shutdown":
            return CollaborationAgentStatusSummary(status: .succeeded, label: normalized.replacingOccurrences(of: "_", with: " "), detail: detail)
        case "errored", "not_found":
            return CollaborationAgentStatusSummary(status: .failed, label: normalized.replacingOccurrences(of: "_", with: " "), detail: detail)
        case "pending_init", "running":
            return CollaborationAgentStatusSummary(status: .inProgress, label: normalized.replacingOccurrences(of: "_", with: " "), detail: detail)
        default:
            return CollaborationAgentStatusSummary(status: .inProgress, label: normalized.replacingOccurrences(of: "_", with: " "), detail: detail)
        }
    }

    private func shortThreadID(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return value
        }

        if trimmed.count <= 8 {
            return trimmed
        }

        return String(trimmed.prefix(8))
    }

    private func summarizeBrowserWorkflow(_ event: BrowserWorkflowEvent) -> String {
        var lines: [String] = [event.headline]
        if !event.detailLines.isEmpty {
            lines.append(contentsOf: event.detailLines.prefix(4))
        }

        if let artifactPreview = event.artifactPreview,
           !artifactPreview.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            lines.append(artifactPreview)
        }

        return lines.joined(separator: "\n")
    }

    private func parseBrowserWorkflowEvent(
        payloadType: String,
        object: [String: JSONValue]
    ) -> BrowserWorkflowEvent? {
        switch payloadType {
        case "web_search_begin":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let query = normalizedBrowserValue(object["query"]?.stringValue)
            var details: [String] = []
            if let query {
                details.append("Query: \"\(query)\"")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }

            let headline = query.map { "Searching web for \"\($0)\"" } ?? "Searching web"
            return BrowserWorkflowEvent(
                status: .inProgress,
                headline: headline,
                detailLines: details,
                artifactPreview: nil,
                callId: callId
            )

        case "web_search_end":
            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            let query = normalizedBrowserValue(object["query"]?.stringValue)
            var details: [String] = []
            if let query {
                details.append("Query: \"\(query)\"")
            }
            if let action = summarizeWebSearchAction(object["action"]?.objectValue) {
                details.append("Action: \(action)")
            }
            if let callId {
                details.append("Call id: \(callId)")
            }

            let headline = query.map { "Search results ready for \"\($0)\"" } ?? "Web search complete"
            return BrowserWorkflowEvent(
                status: .succeeded,
                headline: headline,
                detailLines: details,
                artifactPreview: nil,
                callId: callId
            )

        case "mcp_tool_call_begin", "mcp_tool_call_end":
            guard let invocation = object["invocation"]?.objectValue,
                  let descriptor = describeBrowserInvocation(invocation)
            else {
                return nil
            }

            let callId = normalizedBrowserValue(object["call_id"]?.stringValue)
            var details = descriptor.detailLines
            if let callId {
                details.append("Call id: \(callId)")
            }

            if payloadType == "mcp_tool_call_begin" {
                return BrowserWorkflowEvent(
                    status: .inProgress,
                    headline: descriptor.inProgressHeadline,
                    detailLines: details,
                    artifactPreview: nil,
                    callId: callId
                )
            }

            let mcpResult = summarizeBrowserMcpResult(object["result"])
            return BrowserWorkflowEvent(
                status: mcpResult.status,
                headline: mcpResult.status == .failed
                    ? "\(descriptor.actionLabel) failed"
                    : "\(descriptor.actionLabel) complete",
                detailLines: details,
                artifactPreview: mcpResult.preview,
                callId: callId
            )

        default:
            return nil
        }
    }

    private func describeBrowserInvocation(_ invocation: [String: JSONValue]) -> BrowserInvocationDescriptor? {
        let server = normalizedBrowserValue(invocation["server"]?.stringValue)?.lowercased() ?? ""
        let tool = normalizedBrowserValue(invocation["tool"]?.stringValue)?.lowercased() ?? ""

        let looksBrowserRelated = server.contains("browser")
            || tool.contains("browser")
            || tool.contains("web_search")
            || tool.contains("web-search")

        guard looksBrowserRelated else {
            return nil
        }

        let arguments = invocation["arguments"]?.objectValue
        let actionRaw = normalizedBrowserValue(arguments?["action"]?.stringValue)
            ?? normalizedBrowserValue(invocation["tool"]?.stringValue)
            ?? "browser action"
        let actionLabel = formatBrowserActionLabel(actionRaw)

        var details: [String] = []
        if let url = normalizedBrowserValue(arguments?["url"]?.stringValue) {
            details.append("URL: \(url)")
        }

        if let query = normalizedBrowserValue(arguments?["query"]?.stringValue) {
            details.append("Query: \"\(query)\"")
        }

        if let queries = arguments?["queries"]?.arrayValue {
            let entries = queries
                .compactMap(\.stringValue)
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
            if !entries.isEmpty {
                details.append("Queries: \(entries.prefix(3).joined(separator: " · "))")
            }
        }

        if let pattern = normalizedBrowserValue(arguments?["pattern"]?.stringValue) {
            details.append("Pattern: \"\(pattern)\"")
        }

        if let serverName = normalizedBrowserValue(invocation["server"]?.stringValue),
           let toolName = normalizedBrowserValue(invocation["tool"]?.stringValue) {
            details.append("Tool: \(serverName)/\(toolName)")
        }

        let inProgressHeadline = "\(actionLabel) in progress"
        return BrowserInvocationDescriptor(
            actionLabel: actionLabel,
            inProgressHeadline: inProgressHeadline,
            detailLines: details
        )
    }

    private func summarizeWebSearchAction(_ action: [String: JSONValue]?) -> String? {
        guard let action else {
            return nil
        }

        let type = normalizedBrowserValue(action["type"]?.stringValue)?.lowercased() ?? ""
        switch type {
        case "search":
            if let query = normalizedBrowserValue(action["query"]?.stringValue) {
                return "Search \"\(query)\""
            }

            let queries = action["queries"]?.arrayValue?
                .compactMap(\.stringValue)
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }

            if let queries,
               !queries.isEmpty {
                return "Search \(queries.prefix(3).joined(separator: " · "))"
            }

            return "Search"

        case "open_page":
            if let url = normalizedBrowserValue(action["url"]?.stringValue) {
                return "Open page \(url)"
            }
            return "Open page"

        case "find_in_page":
            var parts: [String] = ["Find in page"]
            if let pattern = normalizedBrowserValue(action["pattern"]?.stringValue) {
                parts.append("\"\(pattern)\"")
            }
            if let url = normalizedBrowserValue(action["url"]?.stringValue) {
                parts.append("at \(url)")
            }
            return parts.joined(separator: " ")

        default:
            if !type.isEmpty {
                return formatBrowserActionLabel(type)
            }
            return nil
        }
    }

    private func summarizeBrowserMcpResult(_ value: JSONValue?) -> BrowserMcpResultSummary {
        guard let resultObject = value?.objectValue else {
            return BrowserMcpResultSummary(status: .succeeded, preview: nil)
        }

        if let errorMessage = normalizedBrowserValue(resultObject["Err"]?.stringValue) {
            return BrowserMcpResultSummary(status: .failed, preview: "Error: \(errorMessage)")
        }

        guard let okObject = resultObject["Ok"]?.objectValue else {
            return BrowserMcpResultSummary(status: .succeeded, preview: nil)
        }

        let isError = okObject["isError"]?.boolValue ?? false
        let preview = summarizeCallToolResultPreview(okObject)
        return BrowserMcpResultSummary(status: isError ? .failed : .succeeded, preview: preview)
    }

    private func summarizeCallToolResultPreview(_ result: [String: JSONValue]) -> String? {
        if let contentItems = result["content"]?.arrayValue {
            let lines = contentItems
                .compactMap { item -> String? in
                    if let text = normalizedBrowserValue(item.stringValue) {
                        return text
                    }

                    guard let object = item.objectValue else {
                        return nil
                    }

                    if let text = normalizedBrowserValue(object["text"]?.stringValue) {
                        return text
                    }

                    if let url = normalizedBrowserValue(object["url"]?.stringValue) {
                        return url
                    }

                    return nil
                }
                .filter { !$0.isEmpty }

            if !lines.isEmpty {
                return truncate(lines.prefix(3).joined(separator: "\n"), maxCharacters: 380)
            }
        }

        if let structuredContent = result["structuredContent"] {
            let pretty = structuredContent.pretty.trimmingCharacters(in: .whitespacesAndNewlines)
            if !pretty.isEmpty,
               pretty != "null" {
                return truncate(pretty, maxCharacters: 380)
            }
        }

        return nil
    }

    private func normalizedBrowserValue(_ value: String?) -> String? {
        guard let value else {
            return nil
        }

        let normalized = normalizeStructuredText(value).trimmingCharacters(in: .whitespacesAndNewlines)
        return normalized.isEmpty ? nil : normalized
    }

    private func formatBrowserActionLabel(_ rawValue: String) -> String {
        let trimmed = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return "Browser action"
        }

        var normalized = trimmed
        if normalized.lowercased().hasPrefix("browser.") {
            normalized.removeFirst("browser.".count)
        }

        normalized = normalized
            .replacingOccurrences(of: "_", with: " ")
            .replacingOccurrences(of: ".", with: " ")

        let words = normalized
            .split(separator: " ")
            .map(String.init)
            .filter { !$0.isEmpty }

        if words.isEmpty {
            return "Browser action"
        }

        let capitalized = words.enumerated().map { index, word in
            if index == 0 {
                return word.prefix(1).uppercased() + word.dropFirst()
            }
            return word
        }

        return capitalized.joined(separator: " ")
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

    private func summarizeRequestUserInput(_ object: [String: JSONValue]) -> String {
        let questionsCount = object["questions"]?.arrayValue?.count ?? 0

        if questionsCount <= 0 {
            return "Awaiting user input"
        }

        if questionsCount == 1 {
            return "Awaiting user input · 1 question"
        }

        return "Awaiting user input · \(questionsCount) questions"
    }

    private func summarizeUserInputAnswer(_ object: [String: JSONValue]) -> String {
        guard let answersObject = object["answers"]?.objectValue else {
            return "User input submitted"
        }

        let questionCount = answersObject.count
        if questionCount <= 0 {
            return "User input submitted · skipped"
        }

        if questionCount == 1 {
            return "User input submitted · 1 response"
        }

        return "User input submitted · \(questionCount) responses"
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

    private func summarizeSystemMessage(_ value: String?) -> String {
        guard let value else {
            return ""
        }

        if isAutoReviewSummaryMessage(value) || isAutoReviewStreamSummaryMessage(value) {
            return summarizeAutoReviewMessage(value)
        }

        return value
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
    case sessionHistoryPage(SessionHistoryPageMessage)
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
        case "session_history_page":
            self = .sessionHistoryPage(try SessionHistoryPageMessage(from: decoder))
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
    let beforeSeq: UInt64?
    let limit: Int?

    static func listSessions(requestId: String) -> Self {
        Self(
            type: "list_sessions",
            requestId: requestId,
            sessionId: nil,
            fromSeq: nil,
            beforeSeq: nil,
            limit: nil
        )
    }

    static func attachSession(requestId: String, sessionId: UUID, fromSeq: UInt64) -> Self {
        Self(
            type: "attach_session",
            requestId: requestId,
            sessionId: sessionId,
            fromSeq: fromSeq,
            beforeSeq: nil,
            limit: nil
        )
    }

    static func loadHistoryBefore(requestId: String, sessionId: UUID, beforeSeq: UInt64, limit: Int) -> Self {
        Self(
            type: "load_history_before",
            requestId: requestId,
            sessionId: sessionId,
            fromSeq: nil,
            beforeSeq: beforeSeq,
            limit: limit
        )
    }

    static func detachSession(requestId: String, sessionId: UUID) -> Self {
        Self(
            type: "detach_session",
            requestId: requestId,
            sessionId: sessionId,
            fromSeq: nil,
            beforeSeq: nil,
            limit: nil
        )
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

struct UserInputAnswerMessage: Encodable {
    let type: String = "user_input_answer"
    let requestId: String
    let sessionId: UUID
    let turnId: String
    let answers: [String: UserInputAnswerPayload]
}

struct UserInputAnswerPayload: Encodable {
    let answers: [String]
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

struct RequestUserInputOption: Hashable {
    let label: String
    let description: String
}

struct RequestUserInputQuestion: Hashable, Identifiable {
    let id: String
    let header: String
    let question: String
    let isOther: Bool
    let isSecret: Bool
    let options: [RequestUserInputOption]
}

struct RequestUserInputPrompt: Hashable {
    let callId: String
    let turnId: String
    let questions: [RequestUserInputQuestion]
}

enum RequestUserInputPromptState: Hashable {
    case ready(RequestUserInputPrompt)
    case loading(callId: String, turnId: String)
    case empty(callId: String, turnId: String)
    case invalid(callId: String, turnId: String, reason: String)

    var callId: String {
        switch self {
        case .ready(let prompt):
            return prompt.callId
        case .loading(let callId, _), .empty(let callId, _), .invalid(let callId, _, _):
            return callId
        }
    }

    var turnId: String {
        switch self {
        case .ready(let prompt):
            return prompt.turnId
        case .loading(_, let turnId), .empty(_, let turnId), .invalid(_, let turnId, _):
            return turnId
        }
    }

    var isResponseActionable: Bool {
        if case .ready = self {
            return true
        }
        return false
    }
}

struct ExecCommandInfo {
    let command: String
    let output: String
    let exitCode: Int?
    let duration: String?
}

enum BrowserWorkflowStatus: Hashable {
    case inProgress
    case succeeded
    case failed
}

enum CollaborationProgressStatus: Hashable {
    case inProgress
    case succeeded
    case failed
}

struct BrowserWorkflowEvent: Hashable {
    let status: BrowserWorkflowStatus
    let headline: String
    let detailLines: [String]
    let artifactPreview: String?
    let callId: String?
}

struct CollaborationProgressEvent: Hashable {
    let status: CollaborationProgressStatus
    let headline: String
    let detailLines: [String]
    let artifactPreview: String?
    let callId: String?
}

private struct BrowserInvocationDescriptor {
    let actionLabel: String
    let inProgressHeadline: String
    let detailLines: [String]
}

private struct BrowserMcpResultSummary {
    let status: BrowserWorkflowStatus
    let preview: String?
}

private struct CollaborationAgentStatusSummary {
    let status: CollaborationProgressStatus
    let label: String
    let detail: String?
}

struct TokenUsageBreakdown {
    let total: Int?
    let input: Int?
    let output: Int?
    let reasoning: Int?
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

enum TaskLifecyclePhase: Hashable {
    case started
    case complete
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

    var isTaskLifecycleEvent: Bool {
        taskLifecyclePhase != nil
    }

    var isExecCommandBeginEvent: Bool {
        guard type == "core_event" else {
            return false
        }
        return event?.payload?.typeHint == "exec_command_begin"
    }

    var isAutoReviewSummaryEvent: Bool {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "agent_message",
              let object = payload.objectValue,
              let message = object["message"]?.stringValue
        else {
            return false
        }

        return isAutoReviewSummaryMessage(message) || isAutoReviewStreamSummaryMessage(message)
    }

    var taskLifecyclePhase: TaskLifecyclePhase? {
        guard type == "core_event",
              let payloadType = event?.payload?.typeHint
        else {
            return nil
        }

        switch payloadType {
        case "task_started":
            return .started
        case "task_complete":
            return .complete
        default:
            return nil
        }
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

    var tokenUsageBreakdown: TokenUsageBreakdown? {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "token_count",
              let object = payload.objectValue,
              let info = object["info"]?.objectValue,
              let usage = info["last_token_usage"]?.objectValue
        else {
            return nil
        }

        let total = usage["total_tokens"]?.numberValue.map(Int.init)
        let input = usage["input_tokens"]?.numberValue.map(Int.init)
        let output = usage["output_tokens"]?.numberValue.map(Int.init)
        let reasoning = usage["reasoning_output_tokens"]?.numberValue.map(Int.init)

        if total == nil,
           input == nil,
           output == nil,
           reasoning == nil {
            return nil
        }

        return TokenUsageBreakdown(total: total, input: input, output: output, reasoning: reasoning)
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
            var trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            if role == .user {
                trimmed = sanitizeUserMessage(trimmed)
            }
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
        if role == .user {
            let sanitized = sanitizeUserMessage(text)
            if sanitized.isEmpty {
                return true
            }

            if isImagePlaceholderMessage(sanitized) {
                return true
            }
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
        let sanitized = sanitizeUserMessage(message)
        return sanitized.isEmpty ? nil : sanitized
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

    var requestUserInputPrompt: RequestUserInputPrompt? {
        guard case .ready(let prompt) = requestUserInputPromptState else {
            return nil
        }
        return prompt
    }

    var collaborationProgressEvent: CollaborationProgressEvent? {
        guard type == "core_event",
              let payload = event?.payload,
              let object = payload.objectValue
        else {
            return nil
        }

        let payloadType = object["type"]?.stringValue ?? payload.typeHint ?? ""
        return parseCollaborationProgressEvent(payloadType: payloadType, object: object)
    }

    var isCollaborationProgressEvent: Bool {
        collaborationProgressEvent != nil
    }

    var browserWorkflowEvent: BrowserWorkflowEvent? {
        guard type == "core_event",
              let payload = event?.payload,
              let object = payload.objectValue
        else {
            return nil
        }

        let payloadType = object["type"]?.stringValue ?? payload.typeHint ?? ""
        return parseBrowserWorkflowEvent(payloadType: payloadType, object: object)
    }

    var isBrowserWorkflowEvent: Bool {
        browserWorkflowEvent != nil
    }

    var requestUserInputPromptState: RequestUserInputPromptState? {
        guard type == "core_event",
              let payload = event?.payload,
              payload.typeHint == "request_user_input",
              let object = payload.objectValue,
              let callId = object["call_id"]?.stringValue
        else {
            return nil
        }

        let turnId = object["turn_id"]?.stringValue ?? callId

        guard let questionsArray = object["questions"]?.arrayValue else {
            return .loading(callId: callId, turnId: turnId)
        }

        if questionsArray.isEmpty {
            return .empty(callId: callId, turnId: turnId)
        }

        var questions: [RequestUserInputQuestion] = []
        questions.reserveCapacity(questionsArray.count)

        for (index, value) in questionsArray.enumerated() {
            guard let question = value.objectValue else {
                return .invalid(
                    callId: callId,
                    turnId: turnId,
                    reason: "Question payload #\(index + 1) is not an object."
                )
            }

            guard let id = question["id"]?.stringValue,
                  !id.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            else {
                return .invalid(
                    callId: callId,
                    turnId: turnId,
                    reason: "Question #\(index + 1) is missing an id."
                )
            }

            guard let header = question["header"]?.stringValue,
                  !header.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            else {
                return .invalid(
                    callId: callId,
                    turnId: turnId,
                    reason: "Question #\(index + 1) is missing a header."
                )
            }

            guard let prompt = question["question"]?.stringValue,
                  !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            else {
                return .invalid(
                    callId: callId,
                    turnId: turnId,
                    reason: "Question #\(index + 1) is missing prompt text."
                )
            }

            let options = question["options"]?.arrayValue?
                .compactMap { optionValue -> RequestUserInputOption? in
                    guard let option = optionValue.objectValue,
                          let label = option["label"]?.stringValue,
                          !label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    else {
                        return nil
                    }

                    let description = option["description"]?.stringValue ?? ""
                    return RequestUserInputOption(label: label, description: description)
                } ?? []

            questions.append(
                RequestUserInputQuestion(
                    id: id,
                    header: header,
                    question: prompt,
                    isOther: question["is_other"]?.boolValue ?? false,
                    isSecret: question["is_secret"]?.boolValue ?? false,
                    options: options
                )
            )
        }

        guard !questions.isEmpty else {
            return .empty(callId: callId, turnId: turnId)
        }

        return .ready(RequestUserInputPrompt(callId: callId, turnId: turnId, questions: questions))
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

    var diffRecoveryPlan: DiffRecoveryPlan? {
        guard let diff = turnDiffText else {
            return nil
        }

        return DiffRecoveryPlan(unifiedDiff: diff)
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

        // Composer updates drive the input box state and should not render as
        // transcript cards.
        if type == "composer" {
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

        if payloadType == "token_count" {
            return true
        }

        if payloadType == "plan_update" {
            return true
        }

        if payloadType == "exec_command_begin" || payloadType == "exec_command_output_delta" {
            return true
        }

        if collaborationProgressEvent != nil {
            return true
        }

        if browserWorkflowEvent != nil {
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
           let message = object["message"]?.stringValue {
            let sanitizedMessage = sanitizeUserMessage(message)
            if sanitizedMessage.isEmpty {
                return true
            }

            if isImagePlaceholderMessage(sanitizedMessage) {
                return true
            }
        }

        return false
    }

    var isOptionalActivityEvent: Bool {
        guard type == "core_event",
              let payloadType = event?.payload?.typeHint
        else {
            return false
        }

        if payloadType == "agent_reasoning_section_break"
            || payloadType == "agent_reasoning"
            || payloadType == "task_started"
            || payloadType == "task_complete"
            || payloadType == "exec_command_begin"
            || payloadType == "background_event"
        {
            return true
        }

        if collaborationProgressEvent != nil {
            return true
        }

        if browserWorkflowEvent != nil {
            return true
        }

        return false
    }

    private func isImagePlaceholderMessage(_ message: String) -> Bool {
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.hasPrefix("[image:") && trimmed.hasSuffix("]")
    }

    private func sanitizeUserMessage(_ message: String) -> String {
        let stripped = stripSystemStatusFooter(from: message)
        return stripped.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func stripSystemStatusFooter(from message: String) -> String {
        let normalized = message.replacingOccurrences(of: "\r\n", with: "\n")
        guard let markerRange = normalized.range(of: "== System Status ==", options: [.caseInsensitive]) else {
            return message
        }

        let prefix = normalized[..<markerRange.lowerBound]
        if !prefix.isEmpty,
           !prefix.hasSuffix("\n") {
            return message
        }

        let footer = normalized[markerRange.lowerBound...]
        let normalizedFooter = footer.lowercased()
        let hasSystemMarker = normalizedFooter.contains("[automatic message added by system]")
        let hasStatusFields = normalizedFooter.contains("\ncwd:")
            && normalizedFooter.contains("\nbranch:")
        guard hasSystemMarker || hasStatusFields else {
            return message
        }

        return String(prefix)
    }

    private func isInternalAssistantStatusMessage(_ message: String) -> Bool {
        let normalized = message
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()

        return normalized.hasPrefix("background shell completed (")
            || normalized.contains("sandbox error: command was killed by a signal")
    }

    private func isAutoReviewSummaryMessage(_ message: String) -> Bool {
        let normalized = message
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()

        return normalized.hasPrefix("[developer] background auto-review completed")
            || normalized.hasPrefix("background auto-review completed")
    }

    private func isAutoReviewStreamSummaryMessage(_ message: String) -> Bool {
        let normalized = message
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()

        if normalized.hasPrefix("[auto-review]") {
            return true
        }

        return normalized.hasPrefix("auto review:") || normalized.contains("auto-review")
    }

    private func summarizeAutoReviewMessage(_ message: String) -> String {
        let trimmed = normalizeStructuredText(message).trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.lowercased().hasPrefix("[auto-review]") {
            let cleaned = trimmed.replacingOccurrences(of: "[auto-review]", with: "", options: [.caseInsensitive])
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if cleaned.isEmpty {
                return "Auto-review completed"
            }
            return "Auto-review summary: \(cleaned)"
        }

        let prefixes = [
            "[developer] background auto-review completed:",
            "background auto-review completed:",
            "[developer] background auto-review completed",
            "background auto-review completed",
        ]

        let lowercased = trimmed.lowercased()
        for prefix in prefixes {
            if lowercased.hasPrefix(prefix) {
                let start = trimmed.index(trimmed.startIndex, offsetBy: prefix.count)
                let remainder = trimmed[start...].trimmingCharacters(in: .whitespacesAndNewlines)
                if remainder.isEmpty {
                    return "Auto-review completed"
                }
                return "Auto-review summary: \(remainder)"
            }
        }

        return "Auto-review summary: \(trimmed)"
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

            if requestUserInputPrompt != nil {
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
            if collaborationProgressEvent != nil {
                return .tool
            }
            if payloadType == "turn_diff" {
                return .tool
            }
            if payloadType == "web_search_begin" || payloadType == "web_search_end" {
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
        isPatchApplyEndEvent || isTokenCountEvent || isBackgroundEvent || isCollaborationProgressEvent
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
