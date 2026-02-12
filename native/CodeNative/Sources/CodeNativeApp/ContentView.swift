import SwiftUI

struct ContentView: View {
    @ObservedObject var store: SessionMirrorStore

    @StateObject private var voiceInput = VoiceInputController()
    @StateObject private var voiceOutput = VoiceOutputController()

    @State private var autoSpeakAssistant: Bool = true
    @State private var autoSubmitVoice: Bool = true
    @State private var lastSpokenItemID: String?

    private var canSendTurns: Bool {
        store.connectionState == .connected && store.selectedSession != nil
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
            }

            HStack(spacing: 8) {
                Button("Connect") {
                    Task {
                        await store.connect()
                    }
                }
                .disabled(store.connectionState != .disconnected)

                Button("Disconnect") {
                    store.disconnect()
                }
                .disabled(store.connectionState == .disconnected)

                Button("Refresh") {
                    Task {
                        await store.refreshSessions()
                    }
                }
                .disabled(store.connectionState != .connected)
            }
            .buttonStyle(.borderedProminent)

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
                            TranscriptCard(item: item)
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

            HStack(spacing: 8) {
                Button("Interrupt") {
                    Task {
                        await store.interruptTurn()
                    }
                }
                .buttonStyle(.bordered)
                .disabled(!canSendTurns)

                Button("Submit") {
                    Task {
                        await store.submitComposer()
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(!canSendTurns || store.composerText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
    }

    private var voicePanel: some View {
        GroupBox("Voice") {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Button(voiceInput.isRecording ? "Stop Recording" : "Start Recording") {
                        handleVoiceRecordToggle()
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(store.selectedSession == nil)

                    Button("Stop Speaking") {
                        voiceOutput.stop()
                    }
                    .buttonStyle(.bordered)
                    .disabled(!voiceOutput.isSpeaking)

                    Toggle("Auto Speak", isOn: $autoSpeakAssistant)
                    Toggle("Auto Submit Voice", isOn: $autoSubmitVoice)
                }

                if !voiceInput.liveTranscript.isEmpty {
                    Text(voiceInput.liveTranscript)
                        .font(.caption)
                        .foregroundStyle(.secondary)
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

    private func handleVoiceRecordToggle() {
        if voiceInput.isRecording {
            let finalText = voiceInput.stopRecording()
            let normalized = finalText.trimmingCharacters(in: .whitespacesAndNewlines)
            store.composerText = finalText

            if autoSubmitVoice, !normalized.isEmpty {
                Task {
                    await store.submitComposer()
                }
            }
            return
        }

        voiceOutput.stop()
        Task {
            await voiceInput.startRecording { text, _ in
                Task { @MainActor in
                    store.composerText = text
                }
            }
        }
    }

    private func handleAssistantSpeech() {
        guard autoSpeakAssistant,
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
        }
        .padding(12)
        .background(Color(nsColor: .textBackgroundColor))
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(Color.secondary.opacity(0.2), lineWidth: 1)
        )
    }
}
