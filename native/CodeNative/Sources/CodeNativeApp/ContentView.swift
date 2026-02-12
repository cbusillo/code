import SwiftUI

#if os(macOS)
import AppKit
#endif

struct ContentView: View {
    @ObservedObject var store: SessionMirrorStore

    @StateObject private var voiceInput = VoiceInputController()
    @StateObject private var voiceOutput = VoiceOutputController()

    @AppStorage("code_native_theme_mode") private var themeModeRaw = AppThemeMode.system.rawValue
    @AppStorage("code_native_thread_density") private var threadDensityRaw = ThreadDensity.comfortable.rawValue
    @AppStorage("code_native_open_destination") private var openDestinationRaw = OpenDestination.finder.rawValue
    @AppStorage("code_native_followup_mode") private var followupModeRaw = FollowupMode.steer.rawValue
    @AppStorage("code_native_multiline_behavior") private var multilineBehaviorRaw = MultilineBehavior.cmdEnter.rawValue
    @AppStorage("code_native_prevent_sleep") private var preventSleep = false
    @AppStorage("code_native_glass_window") private var glassWindow = true
    @AppStorage("code_native_auto_speak") private var autoSpeakAssistant = true
    @AppStorage("code_native_auto_submit_voice") private var autoSubmitVoice = true

    @State private var lastSpokenItemID: String?
    @State private var isPressToTalkActive = false
    @State private var showSettings = false
    @State private var showConnectionPopover = false
    @State private var ideContextEnabled = true

    private var canSendTurns: Bool {
        store.connectionState == .connected && store.selectedSession != nil
    }

    private var selectedThemeMode: AppThemeMode {
        AppThemeMode(rawValue: themeModeRaw) ?? .system
    }

    private var selectedThreadDensity: ThreadDensity {
        ThreadDensity(rawValue: threadDensityRaw) ?? .comfortable
    }

    private var connectionChipColor: Color {
        switch store.connectionState {
        case .connected:
            return Color.green.opacity(0.85)
        case .connecting:
            return Color.orange.opacity(0.9)
        case .disconnected:
            return Color.red.opacity(0.85)
        }
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

    private var repoGroups: [RepoSessionGroup] {
        let grouped = Dictionary(grouping: store.sessions) { session in
            let path = URL(fileURLWithPath: session.cwd)
            let repo = path.lastPathComponent
            return repo.isEmpty ? "workspace" : repo
        }

        return grouped
            .map { RepoSessionGroup(repoName: $0.key, sessions: $0.value.sorted(by: { $0.createdAtUnixMs < $1.createdAtUnixMs })) }
            .sorted(by: { $0.repoName.localizedCaseInsensitiveCompare($1.repoName) == .orderedAscending })
    }

    var body: some View {
        ZStack {
            LinearGradient(
                colors: [
                    Color(red: 0.06, green: 0.07, blue: 0.10),
                    Color(red: 0.05, green: 0.06, blue: 0.08)
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            .ignoresSafeArea()

            HStack(spacing: 0) {
                sidebar
                Divider().overlay(Color.white.opacity(0.08))
                mainPanel
            }
        }
        .preferredColorScheme(selectedThemeMode.colorScheme)
        .sheet(isPresented: $showSettings) {
            NativeSettingsView(
                store: store,
                autoSpeakAssistant: $autoSpeakAssistant,
                autoSubmitVoice: $autoSubmitVoice,
                themeModeRaw: $themeModeRaw,
                threadDensityRaw: $threadDensityRaw,
                openDestinationRaw: $openDestinationRaw,
                followupModeRaw: $followupModeRaw,
                multilineBehaviorRaw: $multilineBehaviorRaw,
                preventSleep: $preventSleep,
                glassWindow: $glassWindow
            )
            .frame(minWidth: 900, minHeight: 620)
        }
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
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 8) {
                Text("Code Native")
                    .font(.headline.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.95))

                ActionRailButton(icon: "square.and.pencil", title: "New thread") {
                    Task {
                        await store.createSession(cwd: nil)
                    }
                }

                ActionRailButton(icon: "clock.arrow.circlepath", title: "Automations") {
                    showSettings = true
                }

                ActionRailButton(icon: "wand.and.stars", title: "Skills") {
                    showSettings = true
                }
            }

            Divider().overlay(Color.white.opacity(0.08))

            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if repoGroups.isEmpty {
                        VStack(alignment: .leading, spacing: 10) {
                            Text("No threads yet")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.white.opacity(0.9))
                            Text("Start your first local thread to populate the sidebar.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Button("Create first thread") {
                                Task {
                                    await store.createSession(cwd: nil)
                                }
                            }
                            .buttonStyle(.borderedProminent)
                        }
                        .padding(10)
                        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    } else {
                        ForEach(repoGroups) { group in
                            VStack(alignment: .leading, spacing: 8) {
                                HStack {
                                    Text(group.repoName)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                    Spacer()
                                    Text("\(group.sessions.count)")
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                }

                                if group.sessions.isEmpty {
                                    Text("No threads")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                } else {
                                    ForEach(group.sessions) { session in
                                        ThreadPill(
                                            session: session,
                                            isSelected: store.selectedSessionID == session.id,
                                            density: selectedThreadDensity
                                        ) {
                                            store.selectedSessionID = session.id
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                .padding(.trailing, 4)
            }

            Spacer(minLength: 8)

            Button {
                showSettings = true
            } label: {
                Label("Settings", systemImage: "gearshape")
                    .font(.subheadline)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
            }
            .buttonStyle(.plain)
            .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .foregroundStyle(.secondary)
        }
        .padding(16)
        .frame(width: 250)
        .background(
            ZStack {
                if glassWindow {
                    RoundedRectangle(cornerRadius: 0)
                        .fill(Color.white.opacity(0.03))
                } else {
                    RoundedRectangle(cornerRadius: 0)
                        .fill(Color.black.opacity(0.35))
                }

                RadialGradient(
                    colors: [Color.white.opacity(0.08), .clear],
                    center: .topLeading,
                    startRadius: 10,
                    endRadius: 420
                )
            }
        )
    }

    private var mainPanel: some View {
        VStack(spacing: 0) {
            topBar

            if let session = store.selectedSession {
                transcriptPanel(session: session)
                composerPanel
            } else {
                emptyState
                composerPanel
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var topBar: some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 2) {
                Text(store.selectedSession.map(sessionTitle(for:)) ?? "No thread selected")
                    .font(.headline.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.95))
                Text(store.selectedSession?.model ?? "")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            statusChip

            TopBarButton(icon: "folder", title: "Open") {
                openSelectedWorkspace()
            }
            .disabled(store.selectedSession == nil)

            TopBarButton(icon: "checklist", title: "Commit") {
                showSettings = true
            }
            .disabled(store.selectedSession == nil)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 14)
        .background(Color.black.opacity(0.28))
        .overlay(alignment: .bottom) {
            Divider().overlay(Color.white.opacity(0.08))
        }
    }

    private var statusChip: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(connectionChipColor)
                .frame(width: 8, height: 8)
            Text(store.statusLine)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color.white.opacity(0.06), in: Capsule())
        .onTapGesture {
            showConnectionPopover.toggle()
        }
        .popover(isPresented: $showConnectionPopover, arrowEdge: .bottom) {
            ConnectionPopover(store: store)
                .frame(width: 360)
                .padding(12)
        }
    }

    private func transcriptPanel(session: SessionSummary) -> some View {
        ScrollView {
            VStack(spacing: 16) {
                if store.selectedSessionItems.isEmpty {
                    Text("Ready. Share what you want me to do next.")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.84))
                        .padding(.top, 140)
                        .frame(maxWidth: .infinity, alignment: .center)
                } else {
                    ForEach(store.selectedSessionItems) { item in
                        TranscriptCard(
                            item: item,
                            onApproval: { approved in
                                handleApproval(item: item, approved: approved)
                            }
                        )
                    }
                }
            }
            .padding(.horizontal, 26)
            .padding(.vertical, 24)
            .frame(maxWidth: 980)
            .frame(maxWidth: .infinity)
        }
    }

    private var composerPanel: some View {
        VStack(spacing: 10) {
            VStack(spacing: 0) {
                ZStack(alignment: .topLeading) {
                    TextEditor(text: $store.composerText)
                        .font(.body)
                        .foregroundStyle(.white.opacity(0.9))
                        .scrollContentBackground(.hidden)
                        .frame(minHeight: 108, maxHeight: 220)
                        .padding(.horizontal, 12)
                        .padding(.top, 10)

                    if store.composerText.isEmpty {
                        Text("Ask for follow-up changes")
                            .font(.title3)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 17)
                            .padding(.top, 18)
                            .allowsHitTesting(false)
                    }
                }

                composerControlRows
            }
            .background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .stroke(Color.white.opacity(0.10), lineWidth: 1)
            )
        }
        .padding(.horizontal, 22)
        .padding(.bottom, 18)
    }

    private var composerControlRows: some View {
        VStack(spacing: 0) {
            Divider().overlay(Color.white.opacity(0.10))

            HStack(spacing: 14) {
                Image(systemName: "plus")
                    .foregroundStyle(.secondary)

                Menu {
                    Text("Model selection will be wired to thread settings")
                } label: {
                    Label("GPT-5.3-Codex", systemImage: "chevron.down")
                }
                .menuStyle(.borderlessButton)

                Menu {
                    ForEach(["Low", "Medium", "High"], id: \.self) { option in
                        Button(option) {}
                    }
                } label: {
                    Label("High", systemImage: "chevron.down")
                }
                .menuStyle(.borderlessButton)

                Toggle(isOn: $ideContextEnabled) {
                    Text("IDE context")
                }
                .toggleStyle(.checkbox)

                Spacer()

                VoiceStatusBadge(label: voiceStateLabel, isActive: voiceInput.isRecording)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)

            HStack(spacing: 10) {
                Menu {
                    Button("Local") {}
                } label: {
                    Label("Local", systemImage: "laptopcomputer")
                }
                .menuStyle(.borderlessButton)

                Menu {
                    Button("on-request") {}
                    Button("on-failure") {}
                    Button("never") {}
                } label: {
                    Label("Default permissions", systemImage: "shield")
                }
                .menuStyle(.borderlessButton)

                Spacer()

                PressToTalkButton(
                    isActive: voiceInput.isRecording,
                    isDisabled: !canSendTurns,
                    onPressChanged: handlePressToTalkChanged
                )

                Button {
                    handleVoiceToggleTap()
                } label: {
                    Image(systemName: voiceInput.isRecording ? "mic.fill" : "mic")
                        .font(.headline)
                }
                .buttonStyle(.plain)
                .foregroundStyle(voiceInput.isRecording ? .red : .secondary)
                .frame(width: 30, height: 30)
                .background(Color.white.opacity(0.06), in: Circle())
                .disabled(!canSendTurns)

                Button {
                    Task {
                        await store.interruptTurn()
                    }
                } label: {
                    Image(systemName: "stop.fill")
                        .font(.caption)
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .frame(width: 30, height: 30)
                .background(Color.white.opacity(0.06), in: Circle())
                .disabled(!canSendTurns)

                Button {
                    Task {
                        await store.submitComposer()
                    }
                } label: {
                    Image(systemName: "arrow.up")
                        .font(.headline.weight(.bold))
                        .foregroundStyle(.black)
                }
                .buttonStyle(.plain)
                .frame(width: 34, height: 34)
                .background(
                    canSubmit ? Color.white : Color.white.opacity(0.4),
                    in: Circle()
                )
                .disabled(!canSubmit)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 14)
            .padding(.bottom, 12)
        }
    }

    private var emptyState: some View {
        VStack {
            Spacer()
            Text("Connect and select a session to start mirroring.")
                .font(.title3.weight(.semibold))
                .foregroundStyle(.secondary)
            Spacer()
        }
    }

    private var canSubmit: Bool {
        canSendTurns && !store.composerText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func sessionTitle(for session: SessionSummary) -> String {
        let repoName = URL(fileURLWithPath: session.cwd).lastPathComponent
        let prefix = session.id.uuidString.prefix(8)
        return "Run task \(prefix) · \(repoName)"
    }

    private func openSelectedWorkspace() {
        guard let session = store.selectedSession else {
            return
        }

        let destination = OpenDestination(rawValue: openDestinationRaw) ?? .finder
        let url = URL(fileURLWithPath: session.cwd)

        #if os(macOS)
        switch destination {
        case .finder, .editor:
            NSWorkspace.shared.open(url)
        }
        #endif
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

        if shouldSubmit && autoSubmitVoice && !normalized.isEmpty {
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
                sessionID: item.sessionID,
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

private struct RepoSessionGroup: Identifiable {
    let id = UUID()
    let repoName: String
    let sessions: [SessionSummary]
}

private enum AppThemeMode: String, CaseIterable, Identifiable {
    case light
    case dark
    case system

    var id: String { rawValue }

    var label: String {
        switch self {
        case .light:
            return "Light"
        case .dark:
            return "Dark"
        case .system:
            return "System"
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .light:
            return .light
        case .dark:
            return .dark
        case .system:
            return nil
        }
    }
}

private enum ThreadDensity: String, CaseIterable, Identifiable {
    case compact
    case comfortable

    var id: String { rawValue }

    var label: String {
        switch self {
        case .compact:
            return "Compact"
        case .comfortable:
            return "Comfortable"
        }
    }

    var rowPadding: CGFloat {
        switch self {
        case .compact:
            return 6
        case .comfortable:
            return 10
        }
    }
}

private enum OpenDestination: String, CaseIterable, Identifiable {
    case finder
    case editor

    var id: String { rawValue }

    var label: String {
        switch self {
        case .finder:
            return "Finder"
        case .editor:
            return "Editor"
        }
    }
}

private enum FollowupMode: String, CaseIterable, Identifiable {
    case queue
    case steer

    var id: String { rawValue }

    var label: String {
        switch self {
        case .queue:
            return "Queue"
        case .steer:
            return "Steer"
        }
    }
}

private enum MultilineBehavior: String, CaseIterable, Identifiable {
    case cmdEnter
    case enter

    var id: String { rawValue }

    var label: String {
        switch self {
        case .cmdEnter:
            return "Cmd+Enter"
        case .enter:
            return "Enter"
        }
    }
}

private struct ActionRailButton: View {
    let icon: String
    let title: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Label(title, systemImage: icon)
                .font(.subheadline)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
        }
        .buttonStyle(.plain)
        .foregroundStyle(.white.opacity(0.88))
        .background(Color.white.opacity(0.03), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
    }
}

private struct ThreadPill: View {
    let session: SessionSummary
    let isSelected: Bool
    let density: ThreadDensity
    let onTap: () -> Void

    private var relativeAge: String {
        let nowMs = UInt64(Date().timeIntervalSince1970 * 1000)
        let elapsed = nowMs > session.createdAtUnixMs ? nowMs - session.createdAtUnixMs : 0
        let minutes = elapsed / 60_000
        if minutes < 1 {
            return "now"
        }
        return "\(minutes)m"
    }

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 8) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(session.id.uuidString.prefix(12))
                        .font(.subheadline.weight(.medium))
                        .lineLimit(1)
                    Text(session.model)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 6)
                Text(relativeAge)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 9)
            .padding(.vertical, density.rowPadding)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(isSelected ? .white : .white.opacity(0.87))
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(isSelected ? Color.white.opacity(0.14) : Color.white.opacity(0.04))
        )
    }
}

private struct TopBarButton: View {
    let icon: String
    let title: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: icon)
                Text(title)
            }
            .font(.subheadline.weight(.medium))
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(Color.white.opacity(0.06), in: Capsule())
        }
        .buttonStyle(.plain)
        .foregroundStyle(.white.opacity(0.9))
    }
}

private struct VoiceStatusBadge: View {
    let label: String
    let isActive: Bool

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(isActive ? Color.green : Color.secondary.opacity(0.7))
                .frame(width: 6, height: 6)
            Text(label)
        }
    }
}

private struct ConnectionPopover: View {
    @ObservedObject var store: SessionMirrorStore

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Session Connection")
                .font(.headline)

            TextField("ws://127.0.0.1:4317/ws", text: $store.endpoint)
                .textFieldStyle(.roundedBorder)

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

            if let error = store.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            } else {
                Text(store.statusLine)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

private struct PressToTalkButton: View {
    let isActive: Bool
    let isDisabled: Bool
    let onPressChanged: (Bool) -> Void

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 9, style: .continuous)
                .fill(isActive ? Color.red : Color.accentColor)

            HStack(spacing: 6) {
                Image(systemName: isActive ? "mic.fill" : "mic")
                Text(isActive ? "Release" : "Hold")
                    .fontWeight(.semibold)
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
        }
        .frame(minWidth: 92, minHeight: 30)
        .opacity(isDisabled ? 0.45 : 1)
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
    }
}

private struct TranscriptCard: View {
    let item: SessionStreamItem
    let onApproval: (Bool) -> Void

    private var cardBackground: Color {
        switch item.cardStyle {
        case .assistant:
            return Color.blue.opacity(0.10)
        case .reasoning:
            return Color.orange.opacity(0.10)
        case .tool:
            return Color.teal.opacity(0.11)
        case .approval:
            return Color.yellow.opacity(0.14)
        case .composer:
            return Color.purple.opacity(0.09)
        case .system:
            return Color.gray.opacity(0.12)
        case .defaultStyle:
            return Color.white.opacity(0.05)
        }
    }

    private var cardBorder: Color {
        switch item.cardStyle {
        case .assistant:
            return Color.blue.opacity(0.34)
        case .reasoning:
            return Color.orange.opacity(0.34)
        case .tool:
            return Color.teal.opacity(0.38)
        case .approval:
            return Color.yellow.opacity(0.52)
        case .composer:
            return Color.purple.opacity(0.32)
        case .system:
            return Color.gray.opacity(0.28)
        case .defaultStyle:
            return Color.white.opacity(0.14)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
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
                .foregroundStyle(.white.opacity(0.93))
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
        .padding(14)
        .background(cardBackground)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .stroke(cardBorder, lineWidth: 1)
        )
    }
}

private enum SettingsCategory: String, CaseIterable, Identifiable {
    case general
    case configuration
    case personalization
    case mcpServers
    case git
    case environments
    case worktrees
    case archivedThreads

    var id: String { rawValue }

    var title: String {
        switch self {
        case .general:
            return "General"
        case .configuration:
            return "Configuration"
        case .personalization:
            return "Personalization"
        case .mcpServers:
            return "MCP servers"
        case .git:
            return "Git"
        case .environments:
            return "Environments"
        case .worktrees:
            return "Worktrees"
        case .archivedThreads:
            return "Archived threads"
        }
    }
}

private struct NativeSettingsView: View {
    @ObservedObject var store: SessionMirrorStore
    @Binding var autoSpeakAssistant: Bool
    @Binding var autoSubmitVoice: Bool
    @Binding var themeModeRaw: String
    @Binding var threadDensityRaw: String
    @Binding var openDestinationRaw: String
    @Binding var followupModeRaw: String
    @Binding var multilineBehaviorRaw: String
    @Binding var preventSleep: Bool
    @Binding var glassWindow: Bool

    @State private var selectedCategory: SettingsCategory = .general

    var body: some View {
        HStack(spacing: 0) {
            List(SettingsCategory.allCases, selection: $selectedCategory) { category in
                Text(category.title)
                    .tag(category)
            }
            .listStyle(.sidebar)
            .frame(width: 220)

            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    Text(selectedCategory.title)
                        .font(.title2.weight(.semibold))

                    switch selectedCategory {
                    case .general:
                        SettingsRow(title: "Open destination", description: "Choose where file actions open by default.") {
                            Picker("", selection: $openDestinationRaw) {
                                ForEach(OpenDestination.allCases) { option in
                                    Text(option.label).tag(option.rawValue)
                                }
                            }
                            .pickerStyle(.segmented)
                            .frame(width: 220)
                        }

                        SettingsRow(title: "Thread density", description: "Control compactness in the thread list.") {
                            Picker("", selection: $threadDensityRaw) {
                                ForEach(ThreadDensity.allCases) { option in
                                    Text(option.label).tag(option.rawValue)
                                }
                            }
                            .pickerStyle(.segmented)
                            .frame(width: 240)
                        }

                        SettingsRow(title: "Prevent sleep while running", description: "Keep your Mac awake while work is active.") {
                            Toggle("", isOn: $preventSleep)
                                .labelsHidden()
                        }

                        SettingsRow(title: "Multiline send", description: "Pick how prompts are submitted.") {
                            Picker("", selection: $multilineBehaviorRaw) {
                                ForEach(MultilineBehavior.allCases) { option in
                                    Text(option.label).tag(option.rawValue)
                                }
                            }
                            .pickerStyle(.segmented)
                            .frame(width: 260)
                        }

                        SettingsRow(title: "Follow-up behavior", description: "Choose whether follow-ups queue or steer running turns.") {
                            Picker("", selection: $followupModeRaw) {
                                ForEach(FollowupMode.allCases) { option in
                                    Text(option.label).tag(option.rawValue)
                                }
                            }
                            .pickerStyle(.segmented)
                            .frame(width: 220)
                        }

                    case .configuration:
                        SettingsRow(title: "Mirror endpoint", description: "WebSocket endpoint used by native clients.") {
                            TextField("ws://127.0.0.1:4317/ws", text: $store.endpoint)
                                .textFieldStyle(.roundedBorder)
                                .frame(width: 320)
                        }

                        SettingsRow(title: "Connection status", description: "Current server status for this workspace.") {
                            Text(store.statusLine)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }

                    case .personalization:
                        SettingsRow(title: "Theme", description: "Select appearance mode for the app.") {
                            Picker("", selection: $themeModeRaw) {
                                ForEach(AppThemeMode.allCases) { option in
                                    Text(option.label).tag(option.rawValue)
                                }
                            }
                            .pickerStyle(.segmented)
                            .frame(width: 240)
                        }

                        SettingsRow(title: "Window style", description: "Use translucent shell treatment for sidebars and cards.") {
                            Toggle("", isOn: $glassWindow)
                                .labelsHidden()
                        }

                        SettingsRow(title: "Auto speak responses", description: "Speak assistant replies when new messages arrive.") {
                            Toggle("", isOn: $autoSpeakAssistant)
                                .labelsHidden()
                        }

                        SettingsRow(title: "Auto submit voice", description: "Submit turn automatically after voice capture stops.") {
                            Toggle("", isOn: $autoSubmitVoice)
                                .labelsHidden()
                        }

                    case .mcpServers:
                        PlaceholderSettingsCard(text: "MCP server management will be wired to the shared Codex config and auth flows.")

                    case .git:
                        PlaceholderSettingsCard(text: "Git defaults, commit templates, and review policies will be surfaced here.")

                    case .environments:
                        PlaceholderSettingsCard(text: "Environment profiles and repo-specific launch actions will appear here.")

                    case .worktrees:
                        PlaceholderSettingsCard(text: "Worktree templates and branch naming rules will be configurable here.")

                    case .archivedThreads:
                        PlaceholderSettingsCard(text: "Archived thread browsing and restore controls will be added here.")
                    }
                }
                .padding(22)
            }
        }
        .background(Color(nsColor: .windowBackgroundColor))
    }
}

private struct SettingsRow<Content: View>: View {
    let title: String
    let description: String
    @ViewBuilder let content: Content

    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.headline)
                Text(description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            content
        }
        .padding(14)
        .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}

private struct PlaceholderSettingsCard: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.subheadline)
            .foregroundStyle(.secondary)
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}
