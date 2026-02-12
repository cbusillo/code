import SwiftUI

struct ContentView: View {
    @ObservedObject var store: SessionMirrorStore

    @StateObject private var voiceInput = VoiceInputController()
    @StateObject private var voiceOutput = VoiceOutputController()

    @State private var autoSpeakAssistant: Bool = true
    @State private var autoSubmitVoice: Bool = true
    @State private var lastSpokenItemID: String?
    @State private var isPressToTalkActive: Bool = false
    @State private var createSessionCwd: String = ""

    private var canSendTurns: Bool {
        store.connectionState == .connected && store.selectedSession != nil
    }

    private var voiceStateLabel: String {
        if voiceInput.isRecording {
            switch voiceInput.transcriptState {
            case .listening:
                return "Listening"
            case .partial:
                return "Partial"
            case .final:
                return "Final"
            case .idle:
                return "Listening"
            }
        }

        switch voiceInput.transcriptState {
        case .final:
            return "Final captured"
        case .partial:
            return "Partial captured"
        case .listening:
            return "Listening"
        case .idle:
            return "Idle"
        }
    }

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            detailPanel
        }
        .navigationSplitViewStyle(.balanced)
        .task {
            if store.connectionState == .disconnected {
                await store.connect()
            }
        }
        .onChange(of: store.selectedSessionItems.last?.id) { _, _ in
            handleAssistantSpeech()
        }
        .onDisappear {
            isPressToTalkActive = false
            _ = voiceInput.stopRecording()
            voiceOutput.stop()
        }
    }

    private var sidebar: some View {
        VStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 6) {
                Text("Session Endpoint")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                TextField("ws://127.0.0.1:4317/ws", text: $store.endpoint)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Session endpoint")
                    .accessibilityHint("WebSocket endpoint for the local session server")
            }

            HStack(spacing: 8) {
                Button("Connect") {
                    Task {
                        await store.connect()
                    }
                }
                .disabled(store.connectionState != .disconnected)
                .accessibilityLabel("Connect to session server")

                Button("Disconnect") {
                    store.disconnect()
                }
                .disabled(store.connectionState == .disconnected)
                .accessibilityLabel("Disconnect from session server")

                Button("Refresh") {
                    Task {
                        await store.refreshSessions()
                    }
                }
                .disabled(store.connectionState != .connected)
                .accessibilityLabel("Refresh session list")
            }
            .buttonStyle(.borderedProminent)

            VStack(alignment: .leading, spacing: 6) {
                Text("Create Session")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                HStack(spacing: 8) {
                    TextField("Optional cwd", text: $createSessionCwd)
                        .textFieldStyle(.roundedBorder)
                        .accessibilityLabel("Session working directory")
                        .accessibilityHint("Optional repository path for new session")

                    Button("Create") {
                        Task {
                            await store.createSession(cwd: createSessionCwd)
                            createSessionCwd = ""
                        }
                    }
                    .disabled(store.connectionState != .connected)
                    .accessibilityLabel("Create session")
                }
            }

            HStack {
                Text("Status")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Text(store.statusLine)
                    .font(.caption)
            }

            if let lastError = store.lastError {
                Text(lastError)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            List(selection: $store.selectedSessionID) {
                Section("Sessions") {
                    ForEach(store.sessions) { session in
                        SessionRow(session: session)
                            .tag(session.id)
                    }
                }
            }
            .accessibilityLabel("Session list")
        }
        .padding(12)
        .navigationTitle("Code Native")
    }

    private var detailPanel: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let session = store.selectedSession {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Session \(session.id.uuidString.prefix(8))")
                        .font(.title3.weight(.semibold))
                    Text("\(session.model) · \(session.cwd)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 10) {
                        ForEach(store.selectedSessionItems) { item in
                            TranscriptCard(
                                item: item,
                                onApproval: { approved in
                                    handleApproval(item: item, approved: approved)
                                }
                            )
                        }
                    }
                    .padding(.vertical, 4)
                }

                composerPanel
                voicePanel
            } else {
                ContentUnavailableView(
                    "No Session Selected",
                    systemImage: "rectangle.stack",
                    description: Text("Connect and select a session to view its live transcript.")
                )
            }
        }
        .padding(16)
    }

    private var composerPanel: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Composer")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .textCase(.uppercase)

            TextEditor(text: $store.composerText)
                .font(.body.monospaced())
                .frame(minHeight: 96, maxHeight: 160)
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .stroke(Color.secondary.opacity(0.2), lineWidth: 1)
                )
                .accessibilityLabel("Composer")

            HStack(spacing: 8) {
                Button("Interrupt") {
                    Task {
                        await store.interruptTurn()
                    }
                }
                .buttonStyle(.bordered)
                .disabled(!canSendTurns)
                .keyboardShortcut("i", modifiers: [.command])
                .accessibilityLabel("Interrupt current turn")

                Button("Submit") {
                    Task {
                        await store.submitComposer()
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(!canSendTurns || store.composerText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .keyboardShortcut(.return, modifiers: [.command])
                .accessibilityLabel("Submit composer")
            }
        }
    }

    private var voicePanel: some View {
        GroupBox("Voice") {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    PressToTalkButton(
                        isActive: voiceInput.isRecording,
                        isDisabled: !canSendTurns,
                        onPressChanged: handlePressToTalkChanged
                    )

                    Button(voiceInput.isRecording ? "Tap to Stop" : "Tap to Record") {
                        handleVoiceToggleTap()
                    }
                    .buttonStyle(.bordered)
                    .disabled(!canSendTurns)
                    .keyboardShortcut(" ", modifiers: [.command, .shift])
                    .accessibilityLabel("Toggle recording")

                    Button("Stop Speaking") {
                        voiceOutput.stop()
                    }
                    .buttonStyle(.bordered)
                    .disabled(!voiceOutput.isSpeaking)

                    Toggle("Auto Speak", isOn: $autoSpeakAssistant)
                        .accessibilityHint("Automatically speak assistant responses")
                    Toggle("Auto Submit Voice", isOn: $autoSubmitVoice)
                        .accessibilityHint("Submit turn when recording stops")
                }

                HStack(spacing: 8) {
                    Text("Voice State")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(voiceStateLabel)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(voiceInput.isRecording ? .green : .secondary)
                }

                if !voiceInput.liveTranscript.isEmpty {
                    Text(voiceInput.liveTranscript)
                        .font(.caption)
                        .foregroundStyle(
                            voiceInput.transcriptState == .final
                                ? .primary
                                : .secondary
                        )
                }

                if let voiceError = voiceInput.lastError {
                    Text(voiceError)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func startVoiceCapture() {
        voiceOutput.stop()
        Task {
            await voiceInput.startRecording { text, _ in
                Task { @MainActor in
                    store.composerText = text
                }
            }
        }
    }

    private func stopVoiceCapture(shouldSubmit: Bool) {
        let finalText = voiceInput.stopRecording()
        let normalized = finalText.trimmingCharacters(in: .whitespacesAndNewlines)
        store.composerText = finalText

        if shouldSubmit, autoSubmitVoice, !normalized.isEmpty {
            Task {
                await store.submitComposer()
            }
        }
    }

    private func handlePressToTalkChanged(isPressed: Bool) {
        guard canSendTurns else {
            if isPressToTalkActive {
                isPressToTalkActive = false
                stopVoiceCapture(shouldSubmit: false)
            }
            return
        }

        if isPressed {
            guard !isPressToTalkActive else {
                return
            }
            isPressToTalkActive = true
            startVoiceCapture()
            return
        }

        guard isPressToTalkActive else {
            return
        }
        isPressToTalkActive = false
        stopVoiceCapture(shouldSubmit: true)
    }

    private func handleVoiceToggleTap() {
        if voiceInput.isRecording {
            isPressToTalkActive = false
            stopVoiceCapture(shouldSubmit: true)
            return
        }

        isPressToTalkActive = true
        startVoiceCapture()
    }

    private func handleApproval(item: SessionStreamItem, approved: Bool) {
        guard let approvalRequest = item.approvalRequest else {
            return
        }

        Task {
            await store.submitApproval(
                callID: approvalRequest.callID,
                type: approvalRequest.type,
                approved: approved
            )
        }
    }

    private func handleAssistantSpeech() {
        guard autoSpeakAssistant,
              !voiceInput.isRecording,
              let lastItem = store.selectedSessionItems.last,
              lastItem.id != lastSpokenItemID,
              let text = lastItem.assistantMessageText
        else {
            return
        }

        lastSpokenItemID = lastItem.id
        voiceOutput.speak(text)
    }
}

private struct PressToTalkButton: View {
    let isActive: Bool
    let isDisabled: Bool
    let onPressChanged: (Bool) -> Void

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(isActive ? Color.red : Color.accentColor)

            HStack(spacing: 8) {
                Image(systemName: isActive ? "mic.fill" : "mic")
                Text(isActive ? "Release to Send" : "Hold to Talk")
                    .fontWeight(.semibold)
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
        }
        .frame(minWidth: 180, maxWidth: 260, minHeight: 34)
        .opacity(isDisabled ? 0.45 : 1)
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .stroke(.white.opacity(0.15), lineWidth: 1)
        )
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { _ in
                    if !isDisabled {
                        onPressChanged(true)
                    }
                }
                .onEnded { _ in
                    onPressChanged(false)
                }
        )
        .onChange(of: isDisabled) { _, disabled in
            if disabled {
                onPressChanged(false)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(isActive ? "Release to send" : "Hold to talk")
        .accessibilityHint("Press and hold to capture voice, then release to submit")
        .accessibilityValue(isActive ? "Recording" : "Idle")
        .accessibilityAddTraits(.isButton)
    }
}

private struct SessionRow: View {
    let session: SessionSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(session.model)
                .font(.subheadline.weight(.semibold))
            Text(session.cwd)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Text(session.id.uuidString)
                .font(.caption2.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }
}

private struct TranscriptCard: View {
    let item: SessionStreamItem
    let onApproval: (Bool) -> Void

    private var cardBackground: Color {
        switch item.cardStyle {
        case .assistant:
            return Color.blue.opacity(0.08)
        case .reasoning:
            return Color.orange.opacity(0.08)
        case .tool:
            return Color.teal.opacity(0.09)
        case .approval:
            return Color.yellow.opacity(0.12)
        case .composer:
            return Color.purple.opacity(0.08)
        case .system:
            return Color.gray.opacity(0.12)
        case .defaultStyle:
            return Color(nsColor: .textBackgroundColor)
        }
    }

    private var cardBorder: Color {
        switch item.cardStyle {
        case .assistant:
            return Color.blue.opacity(0.28)
        case .reasoning:
            return Color.orange.opacity(0.32)
        case .tool:
            return Color.teal.opacity(0.34)
        case .approval:
            return Color.yellow.opacity(0.45)
        case .composer:
            return Color.purple.opacity(0.28)
        case .system:
            return Color.gray.opacity(0.25)
        case .defaultStyle:
            return Color.secondary.opacity(0.2)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(item.title)
                    .font(.caption.weight(.semibold))
                    .textCase(.uppercase)
                    .foregroundStyle(.secondary)
                Spacer()
                Text("#\(item.seq)")
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }

            Text(item.body)
                .font(.body.monospaced())
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)

            if item.approvalRequest != nil {
                HStack(spacing: 8) {
                    Button("Approve") {
                        onApproval(true)
                    }
                    .buttonStyle(.borderedProminent)

                    Button("Deny") {
                        onApproval(false)
                    }
                    .buttonStyle(.bordered)
                }
            }
        }
        .padding(12)
        .background(cardBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(cardBorder, lineWidth: 1)
        )
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(item.title) event")
        .accessibilityValue(item.body)
    }
}
