import SwiftUI

#if os(macOS)
import AppKit
#endif

#if os(iOS)
import UIKit
#endif

private func formatCompactTokenCount(_ value: Int) -> String {
    let absolute = Double(abs(value))
    let sign = value < 0 ? "-" : ""
    if absolute >= 1_000_000 {
        let shortened = String(format: "%.1f", absolute / 1_000_000)
            .replacingOccurrences(of: ".0", with: "")
        return "\(sign)\(shortened)M"
    }
    if absolute >= 1_000 {
        let shortened = String(format: "%.1f", absolute / 1_000)
            .replacingOccurrences(of: ".0", with: "")
        return "\(sign)\(shortened)k"
    }
    return "\(value)"
}

func requestInputShortcutDigit(questionIndex: Int, optionIndex: Int) -> String? {
    guard questionIndex == 0,
          optionIndex >= 0,
          optionIndex < 9
    else {
        return nil
    }

    return String(optionIndex + 1)
}

struct ContentView: View {
    @ObservedObject var store: SessionMirrorStore
    @Environment(\.openURL) private var openURL
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    private static let shortDateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d, h:mm a"
        return formatter
    }()

    private static let timeOnlyFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "h:mm a"
        return formatter
    }()

    private static let longDateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d, yyyy, h:mm a"
        return formatter
    }()

    private static let relativeFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter
    }()

    @StateObject private var voiceInput = VoiceInputController()
    @StateObject private var voiceOutput = VoiceOutputController()

    @AppStorage("code_native_theme_mode") private var themeModeRaw = AppThemeMode.system.rawValue
    @AppStorage("code_native_thread_density") private var threadDensityRaw = ThreadDensity.comfortable.rawValue
    @AppStorage("code_native_session_grouping_mode") private var sessionGroupingModeRaw = SessionRailGroupingMode.repository.rawValue
    @AppStorage("code_native_session_rail_visible_limit") private var sessionRailVisibleLimit = SessionRailDisplayPolicy.defaultVisibleLimit
    @AppStorage("code_native_transcript_density") private var transcriptDensityRaw = TranscriptDensity.comfortable.rawValue
    @AppStorage("code_native_open_destination") private var openDestinationRaw = OpenDestination.finder.rawValue
    @AppStorage("code_native_session_ide_map") private var sessionIDEMapRaw = "{}"
    @AppStorage("code_native_followup_mode") private var followupModeRaw = FollowupMode.steer.rawValue
    @AppStorage("code_native_multiline_behavior") private var multilineBehaviorRaw = MultilineBehavior.cmdEnter.rawValue
    @AppStorage("code_native_prevent_sleep") private var preventSleep = false
    @AppStorage("code_native_glass_window") private var glassWindow = true
    @AppStorage("code_native_auto_speak") private var autoSpeakAssistant = true
    @AppStorage("code_native_auto_submit_voice") private var autoSubmitVoice = true
    @AppStorage("code_native_show_activity_events") private var showActivityEvents = true
    @AppStorage("code_native_ide_context_enabled") private var ideContextEnabled = true
    @AppStorage("code_native_selected_model") private var selectedModel = "GPT-5.3-Codex"
    @AppStorage("code_native_reasoning_level") private var selectedReasoningLevel = "High"
    @AppStorage("code_native_sandbox_mode") private var selectedSandboxMode = "Local"
    @AppStorage("code_native_approval_policy") private var selectedApprovalPolicy = "On request"
    @AppStorage("code_native_default_session_ide") private var defaultSessionIDERaw = SessionIDESelection.systemDefault.rawValue

    @State private var lastSpokenItemID: String?
    @State private var showSettings = false
    @State private var settingsCategory: SettingsCategory = .general
    @State private var showThreadPicker = false
    @State private var threadSearchQuery = ""
    @State private var collapsedSessionRailGroupIDs: Set<String> = []
    @State private var voiceInteractionNotice: String?
    @State private var showConnectionPopover = false
    @State private var activeTranscriptItemID: String?
    @State private var cachedTranscriptItems: [SessionStreamItem] = []
    @State private var taskActivityByStartItemID: [String: [String]] = [:]
    @State private var pendingPrependAnchorItemID: String?
    @State private var composerDraft = ""
    @State private var composerMeasuredHeight: CGFloat = 34
    @State private var showSlashCommandLauncher = false
    @State private var slashCommandQuery = ""
    @State private var showContextPicker = false
    @State private var contextPickerQuery = ""
    @State private var selectedInlineContextPath: String?
    @State private var ideOpenFailureMessage: String?
    @State private var indexedContextRootPath: String?
    @State private var indexedContextFilePaths: [String] = []
    @State private var contextIndexLoading = false
    @FocusState private var composerIsFocused: Bool

    private let transcriptBottomAnchor = "transcript.bottom"
    private let modelOptions = WorkflowSettings.modelOptions
    private let reasoningOptions = WorkflowSettings.reasoningOptions
    private let sandboxOptions = WorkflowSettings.sandboxOptions
    private let approvalPolicyOptions = WorkflowSettings.approvalPolicyOptions

    private var canSendTurns: Bool {
        store.connectionState == .connected && store.selectedSession != nil
    }

    private var latestSessionPayloadType: String? {
        latestCorePayloadType(in: store.selectedSessionItems)
    }

    private var voiceCaptureGuardReason: VoiceCaptureGuardReason? {
        VoiceInteractionPolicy.guardReason(
            connectionState: store.connectionState,
            hasSelectedSession: store.selectedSessionID != nil,
            latestPayloadType: latestSessionPayloadType
        )
    }

    private var canStartVoiceCapture: Bool {
        voiceCaptureGuardReason == nil
    }

    private var canToggleVoiceCapture: Bool {
        voiceInput.isRecording || canStartVoiceCapture
    }

    private var voiceMicAccessibilityHint: String {
        if voiceInput.isRecording {
            return "Stops voice recording and keeps the captured draft in the composer."
        }

        if let voiceCaptureGuardReason {
            return voiceCaptureGuardReason.accessibilityHint
        }

        if autoSubmitVoice {
            return "Starts voice input and auto-submits when recording stops."
        }

        return "Starts voice input and keeps the captured text in the composer."
    }

    private var composerHelperText: String? {
        if canSendTurns {
            return nil
        }

        if store.connectionState != .connected {
            return "Connect to start sending messages"
        }

        if store.selectedSession == nil {
            return "Create or select a thread to continue"
        }

        return nil
    }

    private var composerPrimaryActionTitle: String? {
        if store.connectionState != .connected {
            return "Connect"
        }

        if store.selectedSession == nil {
            return "New thread"
        }

        return nil
    }

    private var hasComposerText: Bool {
        !composerDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var showVoiceBadge: Bool {
        voiceInput.isRecording || voiceInput.transcriptState != .idle
    }

    private var voiceCaptureNoticeText: String? {
        if let voiceInteractionNotice,
           !voiceInteractionNotice.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return voiceInteractionNotice
        }

        guard !voiceInput.isRecording,
              let voiceCaptureGuardReason
        else {
            return nil
        }

        return voiceCaptureGuardReason.helperLabel
    }

    private var lastAssistantResponseText: String? {
        for item in store.selectedSessionItems.reversed() {
            guard item.cardStyle == .assistant else {
                continue
            }

            let trimmed = item.body.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty {
                return trimmed
            }
        }

        return nil
    }

    private var quickStartPrompts: [String] {
        [
            "Build a one-page summary of this repo.",
            "Find the top performance bottleneck and fix it.",
            "Create a clear implementation plan for my next feature."
        ]
    }

    private var isCompactPhoneLayout: Bool {
        #if os(iOS)
        return UIDevice.current.userInterfaceIdiom == .phone && horizontalSizeClass == .compact
        #else
        return false
        #endif
    }

    private var topBarActionButtonSize: CGFloat {
        isCompactPhoneLayout ? 30 : 34
    }

    private var compactSessionMetaLine: String? {
        guard isCompactPhoneLayout,
              let session = store.selectedSession
        else {
            return nil
        }

        if let tokenUsage = selectedSessionTokenUsageSummary {
            return "\(sessionDisplaySubtitle(for: session)) • \(tokenUsage)"
        }

        let eventCount = store.selectedSessionItems.count
        let eventLabel: String
        if eventCount == 0 {
            eventLabel = "Ready"
        } else if eventCount == 1 {
            eventLabel = "1 event"
        } else {
            eventLabel = "\(eventCount) events"
        }
        return "\(sessionDisplaySubtitle(for: session)) • \(eventLabel)"
    }

    private var selectedSessionTokenUsageSummary: String? {
        guard let tokenUsage = store.selectedSessionItems
            .reversed()
            .compactMap(\.tokenUsageBreakdown)
            .first
        else {
            return nil
        }

        var parts: [String] = []
        if let input = tokenUsage.input,
           input > 0 {
            parts.append("In \(formatCompactTokenCount(input))")
        }
        if let output = tokenUsage.output,
           output > 0 {
            parts.append("Out \(formatCompactTokenCount(output))")
        }
        if let reasoning = tokenUsage.reasoning,
           reasoning > 0 {
            parts.append("R \(formatCompactTokenCount(reasoning))")
        }

        if parts.isEmpty,
           let total = tokenUsage.total,
           total > 0 {
            parts.append("Total \(formatCompactTokenCount(total))")
        }

        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    private var topBarSubtitleText: String {
        if let session = store.selectedSession {
            return sessionSubtitleLine(for: session)
        }

        if store.connectionState == .connected {
            if isCompactPhoneLayout {
                return "Ready to start a new thread"
            }
            return "Create or select a thread to start mirroring"
        }

        if isCompactPhoneLayout {
            return "Connect to begin"
        }
        return "Connect to begin mirroring"
    }

    private var transcriptRowSpacing: CGFloat {
        selectedTranscriptDensity.rowSpacing + (isCompactPhoneLayout ? -1 : 0)
    }

    private var compactComposerConfigLabel: String {
        let compactModel = selectedModel
            .replacingOccurrences(of: "-Codex", with: "")
            .replacingOccurrences(of: "-Mini", with: "")
        return "\(compactModel) · \(selectedReasoningLevel)"
    }

    private var usesExpandedTopTitle: Bool {
        isCompactPhoneLayout && store.selectedSessionItems.isEmpty
    }

    private var topBarTitleFont: Font {
        if isCompactPhoneLayout {
            return usesExpandedTopTitle ? .title3.weight(.semibold) : .headline.weight(.semibold)
        }
        return .headline.weight(.semibold)
    }

    private var assistantTranscriptMaxWidth: CGFloat {
        #if os(iOS)
        return isCompactPhoneLayout ? 336 : 560
        #else
        return 760
        #endif
    }

    private var welcomePrimaryFontSize: CGFloat {
        #if os(iOS)
        return isCompactPhoneLayout ? 34 : 42
        #else
        return 42
        #endif
    }

    private var welcomeSecondaryFontSize: CGFloat {
        #if os(iOS)
        return isCompactPhoneLayout ? 26 : 44
        #else
        return 44
        #endif
    }

    private var welcomeSecondaryLineLimit: Int {
        isCompactPhoneLayout ? 2 : 3
    }

    private var welcomePromptCardMinHeight: CGFloat {
        isCompactPhoneLayout ? 90 : 112
    }

    private var welcomePromptCardLineLimit: Int {
        isCompactPhoneLayout ? 2 : 3
    }

    private var shouldUsePromptScroller: Bool {
        #if os(iOS)
        return isCompactPhoneLayout
        #else
        return false
        #endif
    }

    private var compactWelcomeSubtitle: String {
        "Start with a quick prompt or type below"
    }

    private var usesCondensedWelcomeCopy: Bool {
        isCompactPhoneLayout || showsIPadSplitLayout
    }

    private var composerControlForegroundStyle: Color {
        isCompactPhoneLayout ? Color.white.opacity(0.62) : .secondary
    }

    private var selectedThemeMode: AppThemeMode {
        AppThemeMode(rawValue: themeModeRaw) ?? .system
    }

    private var selectedThreadDensity: ThreadDensity {
        ThreadDensity(rawValue: threadDensityRaw) ?? .comfortable
    }

    private var selectedSessionGroupingMode: SessionRailGroupingMode {
        SessionRailGroupingMode(rawValue: sessionGroupingModeRaw) ?? .repository
    }

    private var normalizedSessionRailVisibleLimit: Int {
        SessionRailDisplayPolicy.normalizedVisibleLimit(sessionRailVisibleLimit)
    }

    private var selectedTranscriptDensity: TranscriptDensity {
        TranscriptDensity(rawValue: transcriptDensityRaw) ?? .comfortable
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

    private var runtimeStateColor: Color {
        switch store.selectedSessionRuntimeState {
        case .connected:
            return Color.green.opacity(0.84)
        case .reconnecting:
            return Color.orange.opacity(0.9)
        case .historyLoading:
            return Color.blue.opacity(0.88)
        case .historyComplete:
            return Color.teal.opacity(0.84)
        case .unavailable:
            return Color.red.opacity(0.9)
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

    private var visibleSessions: [SessionSummary] {
        store.sessions.filter { session in
            !isHiddenSession(session) && !store.isSessionUnavailable(session.id)
        }
    }

    private var filteredSessions: [SessionSummary] {
        let query = threadSearchQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else {
            return visibleSessions
        }

        let normalized = query.lowercased()
        return visibleSessions.filter { session in
            let title = sessionDisplayTitle(for: session).lowercased()
            let repo = sessionRepoName(for: session).lowercased()
            let model = sessionDisplaySubtitle(for: session).lowercased()
            return title.contains(normalized) || repo.contains(normalized) || model.contains(normalized)
        }
    }

    private var hiddenSessionCountByRepo: [String: Int] {
        store.sessions.reduce(into: [String: Int]()) { result, session in
            guard isHiddenSession(session) else {
                return
            }

            let key = linkedRepoNameForHiddenSession(session)
            result[key, default: 0] += 1
        }
    }

    private var totalHiddenSessionCount: Int {
        hiddenSessionCountByRepo.values.reduce(0, +)
    }

    private var unavailableSessionCount: Int {
        store.unavailableSessionErrors.count
    }

    private var selectedSessionUnavailableError: String? {
        guard let selectedSession = store.selectedSession else {
            return nil
        }

        return store.unavailableSessionError(for: selectedSession.id)
    }

    private var sessionTitleCounts: [String: Int] {
        filteredSessions.reduce(into: [String: Int]()) { counts, session in
            counts[sessionDisplayTitle(for: session), default: 0] += 1
        }
    }

    private var sessionCountByRepoName: [String: Int] {
        filteredSessions.reduce(into: [String: Int]()) { counts, session in
            counts[sessionRepoName(for: session), default: 0] += 1
        }
    }

    private var sessionRailLayout: SessionRailLayout {
        buildSessionRailLayout(
            sessions: filteredSessions,
            groupingMode: selectedSessionGroupingMode,
            selectedSessionID: store.selectedSessionID,
            visibleLimit: normalizedSessionRailVisibleLimit
        )
    }

    private var sessionRailGroups: [SessionRailGroup] {
        sessionRailLayout.groups
    }

    private var shouldShowRepoHeaders: Bool {
        selectedSessionGroupingMode == .repository && sessionRailGroups.count > 1
    }

    private var welcomeQuickSwitchSessions: [SessionSummary] {
        guard let selectedID = store.selectedSessionID else {
            return []
        }

        return filteredSessions
            .filter { $0.id != selectedID }
            .sorted(by: { sessionActivityUnixMs($0) > sessionActivityUnixMs($1) })
            .prefix(4)
            .map { $0 }
    }

    private var emptyQuickSwitchSessions: [SessionSummary] {
        guard store.selectedSessionID == nil else {
            return []
        }

        return filteredSessions
            .sorted(by: { sessionActivityUnixMs($0) > sessionActivityUnixMs($1) })
            .prefix(4)
            .map { $0 }
    }

    private var showsIPadSplitLayout: Bool {
        #if os(iOS)
        return UIDevice.current.userInterfaceIdiom == .pad && horizontalSizeClass == .regular
        #else
        return false
        #endif
    }

    private var showsDeveloperDiagnostics: Bool {
        #if DEBUG
        return ProcessInfo.processInfo.environment["CODE_NATIVE_DEBUG_UI"] == "1"
        #else
        return false
        #endif
    }

    var body: some View {
        ZStack {
            LinearGradient(
                colors: [
                    Color(red: 0.09, green: 0.10, blue: 0.13),
                    Color(red: 0.06, green: 0.07, blue: 0.09)
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            .ignoresSafeArea()

            #if os(iOS)
            if showsIPadSplitLayout {
                HStack(spacing: 0) {
                    sidebar
                        .frame(width: 300)
                    mainPanel
                }
            } else {
                mainPanel
            }
            #else
            HStack(spacing: 0) {
                sidebar
                mainPanel
            }
            #endif
        }
        .preferredColorScheme(selectedThemeMode.colorScheme)
        .environment(\.openURL, OpenURLAction { url in
            handleOpenURL(url)
        })
        .alert("Unable to Open in IDE", isPresented: ideOpenFailureIsPresented) {
            Button("OK", role: .cancel) {
                ideOpenFailureMessage = nil
            }
        } message: {
            Text(ideOpenFailureMessage ?? "Unknown error")
        }
        .sheet(isPresented: $showSettings) {
            NavigationStack {
                settingsSheetContent
                    .navigationTitle("Settings")
                    #if os(iOS)
                    .navigationBarTitleDisplayMode(.inline)
                    #endif
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Done") {
                                showSettings = false
                            }
                            .keyboardShortcut(.cancelAction)
                            .accessibilityIdentifier("settings.done")
                        }
                    }
            }
            #if os(iOS)
            .presentationDetents([.large])
            .presentationDragIndicator(.visible)
            #endif
        }
        .sheet(isPresented: $showSlashCommandLauncher) {
            SlashCommandLauncherView(
                query: $slashCommandQuery,
                commands: filteredSlashCommands,
                onSelect: { command in
                    handleSlashCommandSelection(command)
                }
            )
            #if os(macOS)
            .frame(minWidth: 560, minHeight: 420)
            #endif
        }
        .sheet(isPresented: $showContextPicker) {
            ContextReferencePickerView(
                query: $contextPickerQuery,
                candidates: filteredContextPickerPaths,
                isLoading: contextIndexLoading,
                onRefresh: {
                    ensureContextIndexLoaded(forceReload: true)
                },
                onSelect: { path in
                    insertContextReference(path: path)
                }
            )
            .onAppear {
                ensureContextIndexLoaded()
            }
            #if os(macOS)
            .frame(minWidth: 620, minHeight: 460)
            #endif
        }
        #if os(iOS)
        .sheet(isPresented: $showThreadPicker) {
            NavigationStack {
                sidebar
                    .navigationTitle("Threads")
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .topBarTrailing) {
                            Button("Done") {
                                showThreadPicker = false
                            }
                        }
                    }
            }
        }
        #endif
        .task {
            if store.connectionState == .disconnected {
                await store.connect()
            }
        }
        .onChange(of: store.selectedSessionItems) { _, _ in
            refreshTranscriptCache()
            handleAssistantSpeech()
            enforceVoiceCapturePolicy(clearTranscript: true)
            if let activeTranscriptItemID,
               !transcriptItems.contains(where: { $0.id == activeTranscriptItemID }) {
                self.activeTranscriptItemID = nil
            }
        }
        .onChange(of: store.selectedSessionID) { _, _ in
            if voiceInput.isRecording {
                stopVoiceCapture(shouldSubmit: false, clearTranscript: true)
            }
            voiceInteractionNotice = nil
            activeTranscriptItemID = nil
            composerDraft = store.composerText
            refreshTranscriptCache()
            ensureContextIndexLoaded(forceReload: true)
            #if os(iOS)
            showThreadPicker = false
            #else
            focusComposerEditor(forceActivateApp: true)
            #endif
        }
        .onChange(of: store.composerText) { _, newValue in
            guard newValue != composerDraft else {
                return
            }

            if !composerIsFocused || newValue.isEmpty {
                composerDraft = newValue
            }
        }
        .onChange(of: composerDraft) { _, newValue in
            if ComposerContextReferenceFormatter.trailingMentionMatch(in: newValue) != nil {
                ensureContextIndexLoaded()
            }
            syncInlineContextSelection()
        }
        .onChange(of: displayedInlineContextPaths) { _, _ in
            syncInlineContextSelection()
        }
        .onChange(of: store.sessions) { _, _ in
            pruneSessionIDEPreferences()
            ensureVisibleSelection()
        }
        .onChange(of: showActivityEvents) { _, _ in
            refreshTranscriptCache()
        }
        .onChange(of: store.connectionState) { _, newState in
            if newState != .connected {
                voiceOutput.stop()
                enforceVoiceCapturePolicy(clearTranscript: true)
            }
        }
        #if os(iOS)
        .onChange(of: showsIPadSplitLayout) { _, isSplit in
            if isSplit {
                showThreadPicker = false
            }
        }
        #endif
        .onDisappear {
            stopVoiceCapture(shouldSubmit: false, clearTranscript: true)
            voiceOutput.stop()
        }
        #if os(macOS)
        .onAppear {
            composerDraft = store.composerText
            refreshTranscriptCache()
            ensureContextIndexLoaded(forceReload: true)
            normalizeWorkflowSettings()
            pruneSessionIDEPreferences()
            focusComposerEditor(forceActivateApp: true)
        }
        #else
        .onAppear {
            composerDraft = store.composerText
            refreshTranscriptCache()
            ensureContextIndexLoaded(forceReload: true)
            normalizeWorkflowSettings()
            pruneSessionIDEPreferences()
        }
        #endif
    }

    @ViewBuilder
    private var settingsSheetContent: some View {
        NativeSettingsView(
            store: store,
            autoSpeakAssistant: $autoSpeakAssistant,
            autoSubmitVoice: $autoSubmitVoice,
            themeModeRaw: $themeModeRaw,
            threadDensityRaw: $threadDensityRaw,
            sessionGroupingModeRaw: $sessionGroupingModeRaw,
            sessionRailVisibleLimit: $sessionRailVisibleLimit,
            transcriptDensityRaw: $transcriptDensityRaw,
            openDestinationRaw: $openDestinationRaw,
            followupModeRaw: $followupModeRaw,
            multilineBehaviorRaw: $multilineBehaviorRaw,
            showActivityEvents: $showActivityEvents,
            ideContextEnabled: $ideContextEnabled,
            selectedModel: $selectedModel,
            selectedReasoningLevel: $selectedReasoningLevel,
            selectedSandboxMode: $selectedSandboxMode,
            selectedApprovalPolicy: $selectedApprovalPolicy,
            defaultSessionIDERaw: $defaultSessionIDERaw,
            preventSleep: $preventSleep,
            glassWindow: $glassWindow,
            initialCategory: settingsCategory
        )
        #if os(macOS)
        .frame(minWidth: 900, minHeight: 620)
        #endif
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 8) {
                Text("Code")
                    .font(.headline.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.95))

                ActionRailButton(
                    icon: "square.and.pencil",
                    title: "New thread",
                    accessibilityID: "rail.new-thread"
                ) {
                    Task {
                        await store.createSession(cwd: nil)
                    }
                }
                #if os(macOS)
                .keyboardShortcut("n", modifiers: [.command])
                #endif

                if showsDeveloperDiagnostics {
                    ActionRailButton(icon: "arrow.clockwise", title: "Refresh", accessibilityID: "rail.refresh") {
                        Task {
                            await store.refreshSessions()
                        }
                    }
                    #if os(macOS)
                    .keyboardShortcut("r", modifiers: [.command])
                    #endif
                }

                ActionRailButton(icon: "clock.arrow.circlepath", title: "Automations", accessibilityID: "rail.automations") {
                    presentSettings(.configuration)
                }
                #if os(macOS)
                .keyboardShortcut(",", modifiers: [.command])
                #endif

                ActionRailButton(icon: "cube.box", title: "Skills", accessibilityID: "rail.skills") {
                    presentSettings(.mcpServers)
                }
            }

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    if !visibleSessions.isEmpty {
                        HStack(spacing: 8) {
                            Image(systemName: "magnifyingglass")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            TextField("Filter threads", text: $threadSearchQuery)
                                .font(.caption)
                                .foregroundStyle(.white.opacity(0.9))
                                .accessibilityIdentifier("sidebar.search")
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 8)
                        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    }

                    if sessionRailGroups.isEmpty {
                        VStack(alignment: .leading, spacing: 10) {
                            Text(threadSearchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "No threads" : "No matching threads")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.white.opacity(0.9))
                            Text(threadSearchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Start your first local thread to populate the sidebar." : "Try a different filter or clear search.")
                                .font(.caption)
                                .foregroundStyle(.secondary)

                            if showsDeveloperDiagnostics && totalHiddenSessionCount > 0 {
                                Text("\(totalHiddenSessionCount) internal threads are hidden.")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }

                            if showsDeveloperDiagnostics && unavailableSessionCount > 0 {
                                Text("\(unavailableSessionCount) unavailable threads are hidden.")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }

                            if threadSearchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                                Button("Create first thread") {
                                    Task {
                                        await store.createSession(cwd: nil)
                                    }
                                }
                                .buttonStyle(.borderedProminent)
                            } else {
                                Button("Clear filter") {
                                    threadSearchQuery = ""
                                }
                                .buttonStyle(.bordered)
                            }
                        }
                        .padding(10)
                        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    } else {
                        HStack(spacing: 8) {
                            Text("Threads")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.white.opacity(0.9))

                            Text("\(sessionRailLayout.totalCount)")
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.white.opacity(0.7))
                                .padding(.horizontal, 6)
                                .padding(.vertical, 2)
                                .background(Color.white.opacity(0.05), in: Capsule(style: .continuous))

                            if showsDeveloperDiagnostics && totalHiddenSessionCount > 0 {
                                Text("\(totalHiddenSessionCount) hidden")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary.opacity(0.9))
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(Color.white.opacity(0.04), in: Capsule(style: .continuous))
                            }

                            if showsDeveloperDiagnostics && unavailableSessionCount > 0 {
                                Text("\(unavailableSessionCount) unavailable")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary.opacity(0.9))
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(Color.white.opacity(0.04), in: Capsule(style: .continuous))
                            }

                            Spacer()

                            Menu {
                                Section("Grouping") {
                                    Picker("Grouping", selection: $sessionGroupingModeRaw) {
                                        ForEach(SessionRailGroupingMode.allCases) { option in
                                            Text(option.label).tag(option.rawValue)
                                        }
                                    }
                                }

                                Section("Density") {
                                    Picker("Thread density", selection: $threadDensityRaw) {
                                        ForEach(ThreadDensity.allCases) { option in
                                            Text(option.label).tag(option.rawValue)
                                        }
                                    }
                                }

                                Section("Visible threads") {
                                    Button("Show \(SessionRailDisplayPolicy.visibleLimitStep) more") {
                                        sessionRailVisibleLimit = SessionRailDisplayPolicy.normalizedVisibleLimit(
                                            normalizedSessionRailVisibleLimit + SessionRailDisplayPolicy.visibleLimitStep
                                        )
                                    }
                                    .disabled(!sessionRailLayout.isTruncated)

                                    Button("Show all") {
                                        sessionRailVisibleLimit = SessionRailDisplayPolicy.maxVisibleLimit
                                    }
                                    .disabled(!sessionRailLayout.isTruncated)

                                    Button("Reset thread cap") {
                                        sessionRailVisibleLimit = SessionRailDisplayPolicy.defaultVisibleLimit
                                    }
                                    .disabled(normalizedSessionRailVisibleLimit == SessionRailDisplayPolicy.defaultVisibleLimit)
                                }
                            } label: {
                                Image(systemName: "slider.horizontal.3")
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(.white.opacity(0.84))
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 6)
                                    .background(Color.white.opacity(0.05), in: Capsule(style: .continuous))
                            }
                            .accessibilityIdentifier("sidebar.rail-controls")
                        }

                        if sessionRailLayout.isTruncated {
                            VStack(alignment: .leading, spacing: 6) {
                                Text("Showing \(sessionRailLayout.visibleCount) of \(sessionRailLayout.totalCount) threads")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)

                                HStack(spacing: 8) {
                                    Button("Show \(min(SessionRailDisplayPolicy.visibleLimitStep, sessionRailLayout.hiddenCount)) more") {
                                        sessionRailVisibleLimit = SessionRailDisplayPolicy.normalizedVisibleLimit(
                                            normalizedSessionRailVisibleLimit + SessionRailDisplayPolicy.visibleLimitStep
                                        )
                                    }
                                    .buttonStyle(.plain)
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(.white.opacity(0.82))
                                    .accessibilityIdentifier("sidebar.show-more")

                                    Button("Show all") {
                                        sessionRailVisibleLimit = SessionRailDisplayPolicy.maxVisibleLimit
                                    }
                                    .buttonStyle(.plain)
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(.white.opacity(0.82))
                                    .accessibilityIdentifier("sidebar.show-all")
                                }
                            }
                            .padding(.horizontal, 10)
                            .padding(.vertical, 8)
                            .background(Color.white.opacity(0.03), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }

                        ForEach(sessionRailGroups) { group in
                            VStack(alignment: .leading, spacing: 8) {
                                if shouldShowRepoHeaders {
                                    let isCollapsed = collapsedSessionRailGroupIDs.contains(group.id)
                                    Button {
                                        if isCollapsed {
                                            collapsedSessionRailGroupIDs.remove(group.id)
                                        } else {
                                            collapsedSessionRailGroupIDs.insert(group.id)
                                        }
                                    } label: {
                                        HStack(spacing: 6) {
                                            Image(systemName: isCollapsed ? "chevron.right" : "chevron.down")
                                                .font(.caption2.weight(.semibold))
                                                .foregroundStyle(.secondary)
                                            Text(group.title)
                                                .font(.caption.weight(.semibold))
                                                .foregroundStyle(.secondary)

                                            Spacer()

                                            Text("\(group.sessions.count)/\(group.totalCount)")
                                                .font(.caption2)
                                                .foregroundStyle(.secondary)

                                            if group.hiddenCount > 0 {
                                                Text("+\(group.hiddenCount)")
                                                    .font(.caption2.weight(.semibold))
                                                    .foregroundStyle(.white.opacity(0.65))
                                                    .padding(.horizontal, 6)
                                                    .padding(.vertical, 2)
                                                    .background(Color.white.opacity(0.05), in: Capsule(style: .continuous))
                                            }
                                        }
                                    }
                                    .buttonStyle(.plain)
                                    .accessibilityIdentifier("sidebar.group.\(group.id)")
                                }

                                if shouldShowRepoHeaders && collapsedSessionRailGroupIDs.contains(group.id) {
                                    EmptyView()
                                } else if group.sessions.isEmpty {
                                    Text("No threads")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                } else {
                                    ForEach(group.sessions) { session in
                                        ThreadPill(
                                            title: sessionSidebarTitle(for: session),
                                            subtitle: sessionSidebarSubtitle(for: session),
                                            trailingLabel: relativeAgeLabel(for: session),
                                            accessibilityID: "session.\(session.id.uuidString)",
                                            isSelected: store.selectedSessionID == session.id,
                                            density: selectedThreadDensity
                                        ) {
                                            store.selectedSessionID = session.id
                                            #if os(iOS)
                                            showThreadPicker = false
                                            #endif
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                .padding(.top, 4)
                .padding(.trailing, 4)
            }
            #if os(iOS)
            .refreshable {
                await store.refreshSessions()
            }
            #endif
            .onChange(of: selectedSessionGroupingMode) { _, mode in
                if mode == .flat {
                    collapsedSessionRailGroupIDs.removeAll()
                }
            }
            .onChange(of: sessionRailGroups.map(\.id)) { _, groupIDs in
                collapsedSessionRailGroupIDs = collapsedSessionRailGroupIDs.intersection(Set(groupIDs))
            }

            Spacer(minLength: 8)

            Button {
                presentSettings(.general)
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
            .accessibilityIdentifier("rail.footer-settings")
        }
        .padding(16)
        #if os(macOS)
        .frame(width: 280)
        #else
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        #endif
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

                Rectangle()
                    .fill(
                        LinearGradient(
                            colors: [.clear, Color.white.opacity(0.05)],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(width: 1)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
        )
    }

    private var mainPanel: some View {
        VStack(spacing: 0) {
            topBar

            if let session = store.selectedSession {
                transcriptPanel(session: session)
            } else {
                emptyState
            }

            if !usesBottomInsetComposer {
                composerPanel
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        #if os(iOS)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if usesBottomInsetComposer {
                composerPanel
            }
        }
        #endif
    }

    private var usesBottomInsetComposer: Bool {
        #if os(iOS)
        return isCompactPhoneLayout
        #else
        return false
        #endif
    }

    private var topBar: some View {
        VStack(alignment: .leading, spacing: isCompactPhoneLayout ? 6 : 8) {
            Text(store.selectedSession.map(sessionTitle(for:)) ?? "New thread")
                .font(topBarTitleFont)
                .foregroundStyle(.white.opacity(0.95))
                .lineLimit(topBarTitleLineLimit)
                .minimumScaleFactor(isCompactPhoneLayout ? 0.74 : 0.85)
                .lineSpacing(isCompactPhoneLayout ? 1 : 0)
                .multilineTextAlignment(.leading)
                .truncationMode(.tail)

            HStack(spacing: 8) {
                if let compactSessionMetaLine {
                    Text(compactSessionMetaLine)
                        .font(.caption2)
                        .foregroundStyle(.white.opacity(0.56))
                        .lineLimit(1)
                        .truncationMode(.tail)
                } else {
                    Text(topBarSubtitleText)
                        .font(.caption)
                        .foregroundStyle(.white.opacity(0.62))
                        .lineLimit(1)
                        .truncationMode(.tail)
                }

                if !isCompactPhoneLayout,
                   let tokenUsage = selectedSessionTokenUsageSummary {
                    HStack(spacing: 5) {
                        Image(systemName: "chart.bar.fill")
                            .font(.caption2.weight(.semibold))
                        Text(tokenUsage)
                            .font(.caption2.weight(.semibold))
                            .lineLimit(1)
                    }
                    .foregroundStyle(.white.opacity(0.82))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color.white.opacity(0.06), in: Capsule(style: .continuous))
                    .overlay(
                        Capsule(style: .continuous)
                            .stroke(Color.white.opacity(0.09), lineWidth: 1)
                    )
                }

                Spacer(minLength: 8)

                if store.connectionState != .connected {
                    statusChip
                }

                topBarActions
            }
        }
        .padding(.horizontal, isCompactPhoneLayout ? 14 : 18)
        .padding(.vertical, isCompactPhoneLayout ? 8 : 10)
        .background(Color.black.opacity(0.24))
    }

    @ViewBuilder
    private var topBarActions: some View {
        #if os(iOS)
        Menu {
            topBarQuickActionMenuItems(includeNavigationItems: isCompactPhoneLayout)
        } label: {
            Image(systemName: "ellipsis.circle")
                .font(.subheadline.weight(.semibold))
                .frame(width: topBarActionButtonSize, height: topBarActionButtonSize)
                .background(Color.white.opacity(0.06), in: Circle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(.white.opacity(0.9))
        .accessibilityIdentifier("top.quick-actions")

        if !showsIPadSplitLayout,
           !isCompactPhoneLayout {
            TopBarIconButton(
                icon: "list.bullet",
                accessibilityID: "top.threads",
                buttonSize: topBarActionButtonSize
            ) {
                showThreadPicker = true
            }
        }

        if !isCompactPhoneLayout {
            TopBarIconButton(
                icon: "gearshape",
                accessibilityID: "top.settings",
                buttonSize: topBarActionButtonSize
            ) {
                presentSettings(.general)
            }
        }
        #else
        if let session = store.selectedSession {
            let availableIDEs = availableSessionIDEs()
            Menu {
                ForEach(availableIDEs) { ide in
                    Button {
                        setSessionIDE(ide, for: session.id)
                    } label: {
                        if ide == selectedIDE(for: session.id) {
                            Label(ide.label, systemImage: "checkmark")
                        } else {
                            Text(ide.label)
                        }
                    }
                }
            } label: {
                Label(selectedIDE(for: session.id).label, systemImage: "chevron.down")
            }
            .menuStyle(.borderlessButton)
        }

        TopBarButton(icon: "folder", title: "Open", accessibilityID: "top.open") {
            openSelectedWorkspace()
        }
        .disabled(store.selectedSession == nil)

        TopBarButton(icon: "circle.dashed.inset.filled", title: "Commit", accessibilityID: "top.review") {
            presentSettings(.git)
        }
        .disabled(store.selectedSession == nil)
        #endif
    }

    @ViewBuilder
    private func topBarQuickActionMenuItems(includeNavigationItems: Bool) -> some View {
        Button {
            Task {
                await store.createSession(cwd: nil)
            }
        } label: {
            Label("New thread", systemImage: "square.and.pencil")
        }
        .accessibilityIdentifier("top.quick-actions.new-thread")

        if showsDeveloperDiagnostics {
            Button {
                Task {
                    await store.refreshSessions()
                }
            } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .accessibilityIdentifier("top.quick-actions.refresh")
        }

        if lastAssistantResponseText != nil {
            Button {
                copyLastAssistantResponseToPasteboard()
            } label: {
                Label("Copy last reply", systemImage: "doc.on.doc")
            }
            .accessibilityIdentifier("top.quick-actions.copy-last")
        }

        if store.connectionState == .disconnected {
            Button {
                Task {
                    await store.connect()
                }
            } label: {
                Label("Reconnect", systemImage: "bolt.horizontal.circle")
            }
            .accessibilityIdentifier("top.quick-actions.reconnect")
        }

        if includeNavigationItems {
            if !showsIPadSplitLayout {
                Divider()

                Button {
                    showThreadPicker = true
                } label: {
                    Label("Threads", systemImage: "list.bullet")
                }
                .accessibilityIdentifier("top.threads")
            }

            Button {
                presentSettings(.general)
            } label: {
                Label("Settings", systemImage: "gearshape")
            }
            .accessibilityIdentifier("top.settings")
        }
    }

    private var topBarTitleLineLimit: Int {
        #if os(iOS)
        if isCompactPhoneLayout {
            return usesExpandedTopTitle ? 2 : 1
        }
        return 1
        #else
        return 1
        #endif
    }

    private var statusChip: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(connectionChipColor)
                .frame(width: 8, height: 8)
            Text(store.selectedSessionRuntimeState.label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(runtimeStateColor)

            Text("•")
                .font(.caption2)
                .foregroundStyle(.secondary)

            Text(store.statusLine)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color.white.opacity(0.06), in: Capsule())
        .onTapGesture {
            showConnectionPopover.toggle()
        }
        .accessibilityIdentifier("top.connection")
        .popover(isPresented: $showConnectionPopover, arrowEdge: .bottom) {
            ConnectionPopover(store: store)
                .frame(width: 360)
                .padding(12)
        }
    }

    private func transcriptPanel(session: SessionSummary) -> some View {
        ScrollViewReader { proxy in
            GeometryReader { geometry in
                ScrollView {
                    LazyVStack(spacing: transcriptRowSpacing) {
                        if transcriptItems.isEmpty {
                            if let unavailableError = selectedSessionUnavailableError {
                                unavailableSessionPanel(for: session, error: unavailableError)
                            } else {
                                welcomePanel(for: session)
                            }
                        } else {
                            let firstItemID = transcriptItems.first?.id
                            Color.clear
                                .frame(height: 1)
                                .id(firstItemID.map { "transcript.top.\($0)" } ?? "transcript.top")
                                .onAppear {
                                    if store.requestOlderHistoryIfNeeded(for: session.id) {
                                        pendingPrependAnchorItemID = transcriptItems.first?.id
                                    }
                                }

                            if store.isLoadingOlderHistory(for: session.id) {
                                HStack(spacing: 8) {
                                    ProgressView()
                                        .controlSize(.small)
                                    Text("Loading earlier history…")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                                .frame(maxWidth: .infinity, alignment: .center)
                                .padding(.vertical, 4)
                            } else if !store.hasMoreHistoryBefore(session.id) {
                                Text("Start of session")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .frame(maxWidth: .infinity, alignment: .center)
                                    .padding(.vertical, 2)
                            }

                            ForEach(Array(transcriptItems.enumerated()), id: \.element.id) { index, item in
                                transcriptRow(item: item)
                                    .onAppear {
                                        if index < 8 {
                                            if store.requestOlderHistoryIfNeeded(for: session.id) {
                                                pendingPrependAnchorItemID = transcriptItems.first?.id
                                            }
                                        }
                                    }
                            }
                        }

                        Color.clear
                            .frame(height: 1)
                            .id(transcriptBottomAnchor)
                    }
                    .padding(.horizontal, transcriptHorizontalPadding)
                    .padding(.vertical, 24)
                    .frame(maxWidth: 920)
                    .frame(maxWidth: .infinity)
                    .frame(minHeight: geometry.size.height, alignment: transcriptContentAlignment)
                }
                .onAppear {
                    scrollTranscriptToBottom(proxy: proxy, animated: false)
                }
                .onChange(of: store.selectedSessionID) { _, _ in
                    pendingPrependAnchorItemID = nil
                    scrollTranscriptToBottom(proxy: proxy, animated: false)
                }
                .onChange(of: store.selectedSessionItems.last?.id) { _, _ in
                    scrollTranscriptToBottom(proxy: proxy, animated: true)
                }
                .onChange(of: transcriptItems.first?.id) { _, newFirstID in
                    guard let anchorID = pendingPrependAnchorItemID,
                          newFirstID != nil,
                          anchorID != newFirstID,
                          transcriptItems.contains(where: { $0.id == anchorID })
                    else {
                        return
                    }

                    pendingPrependAnchorItemID = nil
                    var transaction = Transaction()
                    transaction.disablesAnimations = true
                    withTransaction(transaction) {
                        proxy.scrollTo(anchorID, anchor: .top)
                    }
                }
            }
        }
    }

    private func transcriptRow(item: SessionStreamItem) -> some View {
        let widthCap = transcriptWidthCap(for: item)
        let widthMin = transcriptMinWidth(for: item)
        let shouldFitContentWidth = item.isPatchApplyEndEvent || item.isTokenCountEvent || item.isBackgroundEvent
        let card = TranscriptCard(
            item: item,
            isActive: activeTranscriptItemID == item.id,
            density: selectedTranscriptDensity,
            taskActivityLines: taskActivityByStartItemID[item.id] ?? [],
            onActivate: {
                activeTranscriptItemID = item.id
            },
            onApproval: { decision in
                handleApproval(item: item, decision: decision)
            },
            onRequestUserInputResponse: { answersByQuestionID in
                handleRequestUserInputResponse(item: item, answersByQuestionID: answersByQuestionID)
            }
        )
        .frame(minWidth: widthMin)
        .frame(maxWidth: widthCap, alignment: item.prefersTrailingBubble ? .trailing : .leading)
        .fixedSize(horizontal: shouldFitContentWidth, vertical: false)

        return HStack(spacing: 0) {
            if item.cardStyle == .assistant {
                AssistantTranscriptLine(
                    text: item.body,
                    sessionID: item.sessionId,
                    density: selectedTranscriptDensity,
                    isActive: activeTranscriptItemID == item.id,
                    onActivate: {
                        activeTranscriptItemID = item.id
                    }
                )
                    .frame(maxWidth: assistantTranscriptMaxWidth, alignment: .leading)
                Spacer(minLength: transcriptBubbleGutter)
            } else if item.prefersCenteredBubble {
                Spacer(minLength: transcriptBubbleGutter)
                card
                Spacer(minLength: transcriptBubbleGutter)
            } else {
                if item.prefersTrailingBubble {
                    Spacer(minLength: transcriptBubbleGutter)
                }

                card

                if !item.prefersTrailingBubble {
                    Spacer(minLength: transcriptBubbleGutter)
                }
            }
        }
        .frame(maxWidth: .infinity)
    }

    private var transcriptHorizontalPadding: CGFloat {
        #if os(iOS)
        if showsIPadSplitLayout {
            return selectedTranscriptDensity.horizontalPadding + 4
        }
        return isCompactPhoneLayout
            ? max(8, selectedTranscriptDensity.horizontalPadding - 2)
            : selectedTranscriptDensity.horizontalPadding
        #else
        return selectedTranscriptDensity.horizontalPadding + 12
        #endif
    }

    private var transcriptBubbleGutter: CGFloat {
        #if os(iOS)
        if showsIPadSplitLayout {
            return selectedTranscriptDensity.bubbleGutter + 20
        }
        return isCompactPhoneLayout
            ? max(6, selectedTranscriptDensity.bubbleGutter - 2)
            : selectedTranscriptDensity.bubbleGutter
        #else
        return selectedTranscriptDensity.bubbleGutter + 36
        #endif
    }

    private var transcriptContentAlignment: Alignment {
        #if os(iOS)
        return .top
        #else
        return .bottom
        #endif
    }

    private var transcriptItems: [SessionStreamItem] {
        cachedTranscriptItems
    }

    private func refreshTranscriptCache() {
        let visible = store.selectedSessionItems.filter {
            shouldIncludeInTranscript($0) && !$0.isReplayOmittedNotice
        }
        let replayPruned = pruneReplayHistoryCardsWhenRedundant(in: visible)
        let taskMerged = mergeTaskActivityIntoTaskCards(in: replayPruned)
        taskActivityByStartItemID = taskMerged.activityByStartItemID
        cachedTranscriptItems = dedupeAssistantMessagesWithinTurn(
            in: collapseConsecutiveReasoningCards(
                in: dedupeObsoleteBackgroundEvents(
                    in: collapseTokenUsageBursts(
                        in: removeRedundantPatchSummaries(
                            from: dedupeObsoletePatchApplyCards(in: dedupeObsoleteDiffs(in: taskMerged.items))
                        )
                    )
                )
            )
        )
    }

    private func shouldIncludeInTranscript(_ item: SessionStreamItem) -> Bool {
        if !item.shouldHideFromTranscript {
            return true
        }

        return showActivityEvents && item.isOptionalActivityEvent
    }

    private func pruneReplayHistoryCardsWhenRedundant(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        let hasPrimaryConversation = items.contains { item in
            if item.assistantMessageText != nil {
                return true
            }

            if let userText = item.userMessageText,
               !userText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
               !item.isImagePlaceholderUserMessage {
                return true
            }

            return false
        }

        guard hasPrimaryConversation else {
            return items
        }

        return items.filter { !$0.isReplayHistoryEvent }
    }

    private func collapseTokenUsageBursts(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return items
        }

        var seenTokenModels: Set<String> = []
        var keptReversed: [SessionStreamItem] = []
        keptReversed.reserveCapacity(items.count)

        for item in items.reversed() {
            if item.isConversationBoundaryEvent {
                seenTokenModels.removeAll(keepingCapacity: true)
                keptReversed.append(item)
                continue
            }

            if item.isTokenCountEvent {
                let modelKey = item.tokenCountRequestedModel?.lowercased() ?? "default"
                if seenTokenModels.contains(modelKey) {
                    continue
                }
                seenTokenModels.insert(modelKey)
            }

            keptReversed.append(item)
        }

        return keptReversed.reversed()
    }

    private func dedupeAssistantMessagesWithinTurn(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return items
        }

        var seenAssistantBodies: Set<String> = []
        var keptReversed: [SessionStreamItem] = []
        keptReversed.reserveCapacity(items.count)

        for item in items.reversed() {
            if item.userMessageText != nil {
                seenAssistantBodies.removeAll(keepingCapacity: true)
                keptReversed.append(item)
                continue
            }

            if let currentSignature = assistantMessageSignature(for: item),
               seenAssistantBodies.contains(currentSignature) {
                continue
            }

            if let currentSignature = assistantMessageSignature(for: item) {
                seenAssistantBodies.insert(currentSignature)
            }

            keptReversed.append(item)
        }

        return keptReversed.reversed()
    }

    private func assistantMessageSignature(for item: SessionStreamItem) -> String? {
        guard item.cardStyle == .assistant else {
            return nil
        }

        let normalized = item.body.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty else {
            return nil
        }

        return normalized
    }

    private func removeRedundantPatchSummaries(from items: [SessionStreamItem]) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return items
        }

        var result: [SessionStreamItem] = []
        result.reserveCapacity(items.count)

        for index in items.indices {
            let item = items[index]
            guard item.isPatchApplyEndEvent else {
                result.append(item)
                continue
            }

            var sawDiffSoon = false
            var lookahead = index + 1
            while lookahead < items.endIndex {
                let candidate = items[lookahead]
                if candidate.isConversationBoundaryEvent {
                    break
                }
                if candidate.turnDiffText != nil {
                    sawDiffSoon = true
                    break
                }
                if lookahead - index >= 4 {
                    break
                }
                lookahead += 1
            }

            if !sawDiffSoon {
                result.append(item)
            }
        }

        return result
    }

    private func dedupeObsoleteBackgroundEvents(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        var seenBackgroundBodiesInSegment: Set<String> = []
        var keptReversed: [SessionStreamItem] = []
        keptReversed.reserveCapacity(items.count)

        for item in items.reversed() {
            if item.isConversationBoundaryEvent {
                seenBackgroundBodiesInSegment.removeAll(keepingCapacity: true)
                keptReversed.append(item)
                continue
            }

            if item.isBackgroundEvent {
                let signature = item.body.trimmingCharacters(in: .whitespacesAndNewlines)
                if seenBackgroundBodiesInSegment.contains(signature) {
                    continue
                }
                seenBackgroundBodiesInSegment.insert(signature)
            }

            keptReversed.append(item)
        }

        return keptReversed.reversed()
    }

    private func mergeTaskActivityIntoTaskCards(in items: [SessionStreamItem]) -> (
        items: [SessionStreamItem],
        activityByStartItemID: [String: [String]]
    ) {
        var mergedItems: [SessionStreamItem] = []
        mergedItems.reserveCapacity(items.count)

        var activityByStartItemID: [String: [String]] = [:]
        var activeTaskStartItemID: String?
        var activeTaskSeenLines: Set<String> = []

        for item in items {
            if let phase = item.taskLifecyclePhase {
                switch phase {
                case .started:
                    mergedItems.append(item)
                    activeTaskStartItemID = item.id
                    activeTaskSeenLines.removeAll(keepingCapacity: true)

                case .complete:
                    if let activeTaskStartItemID {
                        appendTaskActivityLines(
                            from: item.body,
                            into: &activityByStartItemID[activeTaskStartItemID, default: []],
                            seenLines: &activeTaskSeenLines
                        )
                    } else {
                        mergedItems.append(item)
                    }

                    activeTaskStartItemID = nil
                    activeTaskSeenLines.removeAll(keepingCapacity: true)
                }

                continue
            }

            if item.isBackgroundEvent,
               let activeTaskStartItemID {
                appendTaskActivityLines(
                    from: item.body,
                    into: &activityByStartItemID[activeTaskStartItemID, default: []],
                    seenLines: &activeTaskSeenLines
                )
                continue
            }

            if item.isExecCommandBeginEvent,
               let activeTaskStartItemID {
                appendTaskActivityLines(
                    from: item.body,
                    into: &activityByStartItemID[activeTaskStartItemID, default: []],
                    seenLines: &activeTaskSeenLines
                )
                mergedItems.append(item)
                continue
            }

            if item.isAutoReviewSummaryEvent,
               let activeTaskStartItemID {
                appendTaskActivityLines(
                    from: item.body,
                    into: &activityByStartItemID[activeTaskStartItemID, default: []],
                    seenLines: &activeTaskSeenLines
                )
            }

            mergedItems.append(item)
        }

        return (items: mergedItems, activityByStartItemID: activityByStartItemID)
    }

    private func appendTaskActivityLines(
        from body: String,
        into lines: inout [String],
        seenLines: inout Set<String>
    ) {
        for line in normalizedStructuredActivityLines(from: body) {
            if seenLines.insert(line).inserted {
                lines.append(line)
            }
        }
    }

    private func collapseConsecutiveReasoningCards(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return items
        }

        var collapsed: [SessionStreamItem] = []
        collapsed.reserveCapacity(items.count)

        for item in items {
            if item.cardStyle == .reasoning,
               let last = collapsed.last,
               last.cardStyle == .reasoning {
                collapsed[collapsed.count - 1] = item
                continue
            }

            collapsed.append(item)
        }

        return collapsed
    }

    private func dedupeObsoletePatchApplyCards(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        var sawPatchApplyInSegment = false
        var keptReversed: [SessionStreamItem] = []
        keptReversed.reserveCapacity(items.count)

        for item in items.reversed() {
            if item.isConversationBoundaryEvent {
                sawPatchApplyInSegment = false
                keptReversed.append(item)
                continue
            }

            if item.isPatchApplyEndEvent {
                if sawPatchApplyInSegment {
                    continue
                }
                sawPatchApplyInSegment = true
            }

            keptReversed.append(item)
        }

        return keptReversed.reversed()
    }

    private func dedupeObsoleteDiffs(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        var seenDiffBodies: Set<String> = []
        var seenDiffKeysInSegment: Set<String> = []
        var keptReversed: [SessionStreamItem] = []
        keptReversed.reserveCapacity(items.count)

        for item in items.reversed() {
            if item.isConversationBoundaryEvent {
                seenDiffKeysInSegment.removeAll(keepingCapacity: true)
                keptReversed.append(item)
                continue
            }

            if let diff = item.turnDiffText {
                let diffKey = primaryDiffPath(in: diff) ?? normalizedDiffSignature(diff)
                if seenDiffKeysInSegment.contains(diffKey) {
                    continue
                }

                let signature = normalizedDiffSignature(diff)
                if seenDiffBodies.contains(signature) {
                    continue
                }

                seenDiffKeysInSegment.insert(diffKey)
                seenDiffBodies.insert(signature)
            }

            keptReversed.append(item)
        }

        return keptReversed.reversed()
    }

    private func normalizedDiffSignature(_ diff: String) -> String {
        let lines = diff.components(separatedBy: "\n")
        var payloadLines: [String] = []
        payloadLines.reserveCapacity(lines.count)

        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.isEmpty || BeautifulDiffPreview.isMetadataLine(line) {
                continue
            }

            payloadLines.append(trimmed)
        }

        let normalized: [String]
        if payloadLines.isEmpty {
            normalized = lines
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
        } else {
            normalized = payloadLines
        }

        return normalized.prefix(120).joined(separator: "\n")
    }

    private func primaryDiffPath(in diff: String) -> String? {
        for line in diff.components(separatedBy: "\n") {
            guard line.hasPrefix("diff --git ") else {
                continue
            }

            let parts = line.split(separator: " ", omittingEmptySubsequences: true)
            guard parts.count >= 4 else {
                continue
            }

            var path = String(parts[3])
            if path.hasPrefix("b/") {
                path.removeFirst(2)
            }
            return path
        }

        return nil
    }

    private func welcomePanel(for session: SessionSummary) -> some View {
        VStack(spacing: isCompactPhoneLayout ? 16 : 22) {
            if !isCompactPhoneLayout {
                Spacer(minLength: showsIPadSplitLayout ? 14 : 34)
            }

            Image(systemName: "sparkles")
                .font(.title2.weight(.semibold))
                .foregroundStyle(.white.opacity(0.9))
                .frame(width: 52, height: 52)
                .background(Color.white.opacity(0.08), in: Circle())

            VStack(spacing: 6) {
                Text("Let's build")
                    .font(.system(size: welcomePrimaryFontSize, weight: .semibold))
                    .foregroundStyle(.white.opacity(0.95))
                if usesCondensedWelcomeCopy {
                    Text(compactWelcomeSubtitle)
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(.white.opacity(0.58))
                        .lineLimit(showsIPadSplitLayout ? 2 : 1)
                        .minimumScaleFactor(0.85)
                        .multilineTextAlignment(.center)
                } else {
                    Text(sessionDisplayTitle(for: session))
                        .font(.system(size: welcomeSecondaryFontSize, weight: .semibold))
                        .foregroundStyle(.white.opacity(0.55))
                        .lineLimit(welcomeSecondaryLineLimit)
                        .minimumScaleFactor(0.75)
                        .multilineTextAlignment(.center)
                }
            }

            welcomePromptSection

            if !welcomeQuickSwitchSessions.isEmpty {
                welcomeRecentThreadsSection
            }

            if !isCompactPhoneLayout {
                Spacer(minLength: showsIPadSplitLayout ? 8 : 24)
            }
        }
        .padding(.top, isCompactPhoneLayout ? 8 : (showsIPadSplitLayout ? 18 : 36))
        .frame(maxWidth: showsIPadSplitLayout ? 900 : .infinity)
        .frame(maxWidth: .infinity, alignment: .top)
    }

    private func unavailableSessionPanel(for session: SessionSummary, error: String) -> some View {
        VStack(spacing: isCompactPhoneLayout ? 14 : 18) {
            if !isCompactPhoneLayout {
                Spacer(minLength: showsIPadSplitLayout ? 10 : 30)
            }

            Image(systemName: "exclamationmark.triangle.fill")
                .font(.title3.weight(.semibold))
                .foregroundStyle(.orange.opacity(0.9))
                .frame(width: 52, height: 52)
                .background(Color.orange.opacity(0.15), in: Circle())

            VStack(spacing: 8) {
                Text("Could not load this thread")
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.95))
                Text(sessionDisplayTitle(for: session))
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.62))
                    .lineLimit(2)
                    .multilineTextAlignment(.center)
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(4)
                    .multilineTextAlignment(.center)
            }

            Button("Start a new thread") {
                Task {
                    await store.createSession(cwd: nil)
                }
            }
            .buttonStyle(.borderedProminent)

            if !isCompactPhoneLayout {
                Spacer(minLength: showsIPadSplitLayout ? 8 : 22)
            }
        }
        .padding(.top, isCompactPhoneLayout ? 8 : (showsIPadSplitLayout ? 16 : 32))
        .frame(maxWidth: showsIPadSplitLayout ? 760 : .infinity)
        .frame(maxWidth: .infinity, alignment: .top)
    }

    @ViewBuilder
    private var welcomePromptSection: some View {
        if shouldUsePromptScroller {
            TabView {
                ForEach(quickStartPrompts, id: \.self) { prompt in
                    QuickPromptCard(
                        prompt: prompt,
                        lineLimit: welcomePromptCardLineLimit,
                        minHeight: welcomePromptCardMinHeight
                    ) {
                        composerDraft = prompt
                    }
                    .padding(.horizontal, 2)
                }
            }
            #if os(iOS)
            .tabViewStyle(.page(indexDisplayMode: .automatic))
            #endif
            .frame(height: welcomePromptCardMinHeight + 12)
            .frame(maxWidth: .infinity)
        } else {
            HStack(spacing: 12) {
                ForEach(quickStartPrompts, id: \.self) { prompt in
                    QuickPromptCard(
                        prompt: prompt,
                        lineLimit: welcomePromptCardLineLimit,
                        minHeight: welcomePromptCardMinHeight
                    ) {
                        composerDraft = prompt
                    }
                }
            }
            .frame(maxWidth: 900)
        }
    }

    private var welcomeRecentThreadsSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Recent threads")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.7))
                Spacer(minLength: 8)
            }

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(welcomeQuickSwitchSessions) { session in
                        Button {
                            store.selectedSessionID = session.id
                        } label: {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(sessionSidebarTitle(for: session))
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(.white.opacity(0.9))
                                    .lineLimit(1)
                                Text(relativeAgeLabel(for: session))
                                    .font(.caption2)
                                    .foregroundStyle(.white.opacity(0.55))
                                    .lineLimit(1)
                            }
                            .frame(width: 180, alignment: .leading)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 8)
                            .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .stroke(Color.white.opacity(0.07), lineWidth: 1)
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 2)
            }
        }
        .frame(maxWidth: 900)
    }

    private var composerPanel: some View {
        VStack(spacing: 10) {
            VStack(spacing: 0) {
                ZStack(alignment: .topLeading) {
                    #if os(macOS)
                    MacComposerTextView(
                        text: $composerDraft,
                        isFocused: composerIsFocused,
                        measuredHeight: $composerMeasuredHeight,
                        onFocusChange: { composerIsFocused = $0 },
                        onSubmit: { submitComposerAction(text: $0) },
                        onEditorKeyCommand: handleComposerKeyCommand
                    )
                    .frame(height: composerEditorHeight)
                    .padding(.horizontal, 12)
                    .padding(.top, 4)
                    .accessibilityIdentifier("composer.input")
                    #else
                    TextEditor(text: $composerDraft)
                        .font(.body)
                        .foregroundStyle(.white.opacity(0.9))
                        .scrollContentBackground(.hidden)
                        .frame(height: composerEditorHeight)
                        .focused($composerIsFocused)
                        .padding(.horizontal, 12)
                        .padding(.top, isCompactPhoneLayout ? 6 : 8)
                        .accessibilityIdentifier("composer.input")
                    #endif

                    if composerDraft.isEmpty {
                        Text("Ask for follow-up changes")
                            .font(.subheadline)
                            .foregroundStyle(.white.opacity(0.42))
                            .padding(.horizontal, isCompactPhoneLayout ? 16 : 18)
                            .padding(.top, isCompactPhoneLayout ? 12 : 10)
                            .allowsHitTesting(false)
                    }
                }
                .contentShape(Rectangle())
                #if os(macOS)
                .onTapGesture {
                    focusComposerEditor(forceActivateApp: true)
                }
                #endif

                if showsInlineContextSuggestions {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Text("Context matches")
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.secondary)
                            Text("Tab inserts")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                            Spacer(minLength: 6)
                            Button("Open picker") {
                                showContextPicker = true
                            }
                            .buttonStyle(.plain)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.white.opacity(0.82))
                        }

                        ForEach(displayedInlineContextPaths, id: \.self) { path in
                            Button {
                                selectedInlineContextPath = path
                                insertContextReference(path: path)
                            } label: {
                                HStack(spacing: 8) {
                                    Image(systemName: "at")
                                        .font(.caption2.weight(.semibold))
                                        .foregroundStyle(.secondary)
                                    Text(path)
                                        .font(.caption.monospaced())
                                        .foregroundStyle(.white.opacity(0.9))
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                    Spacer(minLength: 0)
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 7)
                                .background(
                                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                                        .fill(selectedInlineContextPath == path ? Color.accentColor.opacity(0.18) : Color.white.opacity(0.05))
                                )
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Insert \(path)")
                        }
                    }
                    .padding(.horizontal, 12)
                    .padding(.top, 4)
                    .padding(.bottom, 2)
                    .transition(.opacity.combined(with: .move(edge: .bottom)))
                }

                composerControlRows
            }
            .background(isCompactPhoneLayout ? Color.white.opacity(0.065) : Color.white.opacity(0.05))
            .clipShape(isCompactPhoneLayout ? AnyShape(TopRoundedPanelShape(radius: 20)) : AnyShape(RoundedRectangle(cornerRadius: 18, style: .continuous)))
            .overlay {
                if isCompactPhoneLayout {
                    TopRoundedPanelOutlineShape(radius: 20)
                    .stroke(Color.white.opacity(0.14), lineWidth: 1)
                } else {
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .stroke(Color.white.opacity(0.09), lineWidth: 1)
                }
            }
            .overlay(alignment: .top) {
                if let activity = composerActivityBadge {
                    Label(activity.label, systemImage: activity.icon)
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(activity.tint)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(Color.black.opacity(0.46), in: Capsule(style: .continuous))
                        .overlay(
                            Capsule(style: .continuous)
                                .stroke(activity.tint.opacity(0.38), lineWidth: 1)
                        )
                        .offset(y: -11)
                }
            }
        }
        .padding(.horizontal, composerHorizontalPadding)
        .padding(.bottom, composerBottomPadding)
        .background(alignment: .bottom) {
            if isCompactPhoneLayout {
                Rectangle()
                    .fill(Color.white.opacity(0.065))
                    .frame(height: compactComposerBottomFillHeight)
                    .offset(y: compactComposerBottomFillHeight)
                    .ignoresSafeArea(.container, edges: .bottom)
            }
        }
    }

    private var composerHorizontalPadding: CGFloat {
        #if os(iOS)
        if isCompactPhoneLayout {
            return 0
        }
        return showsIPadSplitLayout ? 16 : 22
        #else
        return 22
        #endif
    }

    private var composerBottomPadding: CGFloat {
        #if os(iOS)
        if isCompactPhoneLayout {
            return 0
        }
        return showsIPadSplitLayout ? 8 : 10
        #else
        return 18
        #endif
    }

    private var composerControlBottomPadding: CGFloat {
        isCompactPhoneLayout ? 2 : 4
    }

    private var compactComposerBottomFillHeight: CGFloat {
        isCompactPhoneLayout ? 34 : 0
    }

    private var composerControlRows: some View {
        #if os(iOS)
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                if isCompactPhoneLayout {
                    Menu {
                        Section("Model") {
                            ForEach(modelOptions, id: \.self) { option in
                                Button(option) {
                                    selectedModel = option
                                }
                            }
                        }

                        Section("Reasoning") {
                            ForEach(reasoningOptions, id: \.self) { option in
                                Button(option) {
                                    selectedReasoningLevel = option
                                }
                            }
                        }
                    } label: {
                        Label(compactComposerConfigLabel, systemImage: "slider.horizontal.3")
                    }
                    .menuStyle(.borderlessButton)
                } else {
                    Menu {
                        ForEach(modelOptions, id: \.self) { option in
                            Button(option) {
                                selectedModel = option
                            }
                        }
                    } label: {
                        Label(selectedModel, systemImage: "chevron.down")
                    }
                    .menuStyle(.borderlessButton)

                    Menu {
                        ForEach(reasoningOptions, id: \.self) { option in
                            Button(option) {
                                selectedReasoningLevel = option
                            }
                        }
                    } label: {
                        Label("Reasoning: \(selectedReasoningLevel)", systemImage: "chevron.down")
                    }
                    .menuStyle(.borderlessButton)
                }

                if isCompactPhoneLayout {
                    Button {
                        showSlashCommandLauncher = true
                    } label: {
                        Image(systemName: "command")
                            .font(.caption.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.white.opacity(0.66))
                    .frame(width: 22, height: 22)
                    .background(Color.white.opacity(0.05), in: Circle())
                    .accessibilityIdentifier("composer.slash")
                    .accessibilityLabel("Slash commands")

                    Button {
                        ensureContextIndexLoaded()
                        showContextPicker = true
                    } label: {
                        Image(systemName: "at")
                            .font(.caption.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.white.opacity(0.66))
                    .frame(width: 22, height: 22)
                    .background(Color.white.opacity(0.05), in: Circle())
                    .accessibilityIdentifier("composer.context")
                    .accessibilityLabel("Insert context reference")

                    Spacer(minLength: 6)

                    Button {
                        handleVoiceToggleTap()
                    } label: {
                        Image(systemName: voiceInput.isRecording ? "mic.fill" : "mic")
                            .font(.caption.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(voiceInput.isRecording ? .red : .white.opacity(0.66))
                    .frame(width: 22, height: 22)
                    .background(Color.white.opacity(0.05), in: Circle())
                    .disabled(!canToggleVoiceCapture)
                    .accessibilityIdentifier("composer.mic-toggle")
                    .accessibilityLabel(voiceInput.isRecording ? "Stop recording" : "Start voice input")
                    .accessibilityHint(voiceMicAccessibilityHint)

                    Button {
                        interruptTurnAction()
                    } label: {
                        Image(systemName: "stop.fill")
                            .font(.caption2.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.white.opacity(0.62))
                    .frame(width: 22, height: 22)
                    .background(Color.white.opacity(0.05), in: Circle())
                    .disabled(!canSendTurns)
                    .accessibilityIdentifier("composer.stop")
                    .accessibilityLabel("Stop response")

                    if hasComposerText {
                        Button {
                            composerDraft = ""
                        } label: {
                            Image(systemName: "xmark")
                                .font(.caption2.weight(.semibold))
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(.white.opacity(0.62))
                        .frame(width: 22, height: 22)
                        .background(Color.white.opacity(0.05), in: Circle())
                        .accessibilityIdentifier("composer.clear")
                        .accessibilityLabel("Clear draft")
                    }

                    Button {
                        submitComposerAction()
                    } label: {
                        Image(systemName: "arrow.up")
                            .font(.caption.weight(.bold))
                            .foregroundStyle(canSubmit ? .black : .white.opacity(0.6))
                    }
                    .buttonStyle(.plain)
                    .frame(width: 26, height: 26)
                    .background(
                        canSubmit ? Color.white : Color.white.opacity(0.24),
                        in: Circle()
                    )
                    .disabled(!canSubmit)
                    .accessibilityIdentifier("composer.send")
                    .accessibilityLabel("Send message")
                } else {
                    Button {
                        showSlashCommandLauncher = true
                    } label: {
                        Label("Slash", systemImage: "command")
                    }
                    .buttonStyle(.plain)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.72))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color.white.opacity(0.06), in: Capsule(style: .continuous))
                    .accessibilityIdentifier("composer.slash")

                    Button {
                        ensureContextIndexLoaded()
                        showContextPicker = true
                    } label: {
                        Label("Context", systemImage: "at")
                    }
                    .buttonStyle(.plain)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.72))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color.white.opacity(0.06), in: Capsule(style: .continuous))
                    .accessibilityIdentifier("composer.context")

                    Spacer(minLength: 10)
                }
            }
            .font(.caption)
            .foregroundStyle(composerControlForegroundStyle)
            .padding(.horizontal, isCompactPhoneLayout ? 16 : 12)
            .padding(.top, isCompactPhoneLayout ? 5 : 8)
            .padding(.bottom, isCompactPhoneLayout ? 3 : 4)

            if let helper = composerHelperText {
                HStack(spacing: 8) {
                    Text(helper)
                        .font(.caption2)
                        .foregroundStyle(.white.opacity(0.62))
                        .lineLimit(1)

                    Spacer(minLength: 8)

                    if let primaryActionTitle = composerPrimaryActionTitle {
                        Button(primaryActionTitle) {
                            composerPrimaryAction()
                        }
                        .buttonStyle(.plain)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.9))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(Color.white.opacity(0.08), in: Capsule(style: .continuous))
                        .accessibilityIdentifier("composer.primary-action")
                    }
                }
                .padding(.horizontal, 12)
                .padding(.bottom, isCompactPhoneLayout ? 1 : 2)
            }

            if let voiceCaptureNoticeText {
                HStack(spacing: 8) {
                    Image(systemName: "mic.slash")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.66))
                    Text(voiceCaptureNoticeText)
                        .font(.caption2)
                        .foregroundStyle(.white.opacity(0.66))
                        .lineLimit(1)
                    Spacer(minLength: 8)
                }
                .padding(.horizontal, 12)
                .padding(.bottom, isCompactPhoneLayout ? 1 : 2)
                .accessibilityIdentifier("composer.voice-notice")
            }

            if !isCompactPhoneLayout {
                HStack(spacing: 8) {
                    Button {
                        handleVoiceToggleTap()
                    } label: {
                        Image(systemName: voiceInput.isRecording ? "mic.fill" : "mic")
                            .font(.caption.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(voiceInput.isRecording ? .red : .white.opacity(0.66))
                    .frame(width: 24, height: 24)
                    .background(Color.white.opacity(0.05), in: Circle())
                    .disabled(!canToggleVoiceCapture)
                    .accessibilityIdentifier("composer.mic-toggle")
                    .accessibilityLabel(voiceInput.isRecording ? "Stop recording" : "Start voice input")
                    .accessibilityHint(voiceMicAccessibilityHint)

                    Button {
                        interruptTurnAction()
                    } label: {
                        Image(systemName: "stop.fill")
                            .font(.caption2.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.white.opacity(0.62))
                    .frame(width: 24, height: 24)
                    .background(Color.white.opacity(0.05), in: Circle())
                    .disabled(!canSendTurns)
                    .accessibilityIdentifier("composer.stop")
                    .accessibilityLabel("Stop response")
                    #if os(macOS)
                    .keyboardShortcut(".", modifiers: [.command])
                    #endif

                    if hasComposerText {
                        Button {
                            composerDraft = ""
                        } label: {
                            Image(systemName: "xmark")
                                .font(.caption2.weight(.semibold))
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(.white.opacity(0.62))
                        .frame(width: 24, height: 24)
                        .background(Color.white.opacity(0.05), in: Circle())
                        .accessibilityIdentifier("composer.clear")
                        .accessibilityLabel("Clear draft")
                    }

                    Spacer(minLength: 6)

                    if showVoiceBadge {
                        VoiceStatusBadge(label: voiceStateLabel, isActive: voiceInput.isRecording)
                            .font(.caption2)
                    }

                    Button {
                        submitComposerAction()
                    } label: {
                        Image(systemName: "arrow.up")
                            .font(.caption.weight(.bold))
                            .foregroundStyle(canSubmit ? .black : .white.opacity(0.6))
                    }
                    .buttonStyle(.plain)
                    .frame(width: 28, height: 28)
                    .background(
                        canSubmit ? Color.white : Color.white.opacity(0.24),
                        in: Circle()
                    )
                    .disabled(!canSubmit)
                    .accessibilityIdentifier("composer.send")
                    .accessibilityLabel("Send message")
                    #if os(macOS)
                    .keyboardShortcut(.return, modifiers: [.command])
                    #endif
                }
                .font(.caption)
                .foregroundStyle(composerControlForegroundStyle)
                .padding(.horizontal, 12)
                .padding(.top, 2)
                .padding(.bottom, composerControlBottomPadding)
            }
        }
        #else
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "plus")
                    .font(.headline)
                    .foregroundStyle(.secondary)
                    .frame(width: 24, height: 24)

                Menu {
                    ForEach(modelOptions, id: \.self) { option in
                        Button(option) {
                            selectedModel = option
                        }
                    }
                } label: {
                    Label(selectedModel, systemImage: "chevron.down")
                }
                .menuStyle(.borderlessButton)

                Menu {
                    ForEach(reasoningOptions, id: \.self) { option in
                        Button(option) {
                            selectedReasoningLevel = option
                        }
                    }
                } label: {
                    Label(selectedReasoningLevel, systemImage: "chevron.down")
                }
                .menuStyle(.borderlessButton)

                Button {
                    showSlashCommandLauncher = true
                } label: {
                    Label("Slash", systemImage: "command")
                }
                .buttonStyle(.borderless)
                .accessibilityIdentifier("composer.slash")
                #if os(macOS)
                .keyboardShortcut("k", modifiers: [.command])
                #endif

                Button {
                    ensureContextIndexLoaded()
                    showContextPicker = true
                } label: {
                    Label("Context", systemImage: "at")
                }
                .buttonStyle(.borderless)
                .accessibilityIdentifier("composer.context")
                #if os(macOS)
                .keyboardShortcut("p", modifiers: [.command, .shift])
                #endif

                #if os(macOS)
                Toggle(isOn: $ideContextEnabled) {
                    Text("IDE context")
                }
                .toggleStyle(.checkbox)
                #else
                Toggle(isOn: $ideContextEnabled) {
                    Text("IDE context")
                }
                #endif

                Button {
                    showActivityEvents.toggle()
                } label: {
                    Label(showActivityEvents ? "Activity on" : "Activity off", systemImage: showActivityEvents ? "waveform.path.ecg" : "waveform.path.ecg.rectangle")
                }
                .buttonStyle(.borderless)

                Spacer()

                Button {
                    handleVoiceToggleTap()
                } label: {
                    Image(systemName: voiceInput.isRecording ? "mic.fill" : "mic")
                        .font(.caption.weight(.semibold))
                }
                .buttonStyle(.plain)
                .foregroundStyle(voiceInput.isRecording ? .red : .secondary)
                .frame(width: 24, height: 24)
                .background(Color.white.opacity(0.05), in: Circle())
                .disabled(!canToggleVoiceCapture)
                .accessibilityIdentifier("composer.mic-toggle")
                .accessibilityLabel(voiceInput.isRecording ? "Stop recording" : "Start voice input")
                .accessibilityHint(voiceMicAccessibilityHint)

                Button {
                    interruptTurnAction()
                } label: {
                    Image(systemName: "stop.fill")
                        .font(.caption2.weight(.semibold))
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 24)
                .background(Color.white.opacity(0.05), in: Circle())
                .disabled(!canSendTurns)
                .accessibilityIdentifier("composer.stop")
                .accessibilityLabel("Stop response")
                #if os(macOS)
                .keyboardShortcut(".", modifiers: [.command])
                #endif

                Button {
                    submitComposerAction()
                } label: {
                    Image(systemName: "arrow.up")
                        .font(.caption.weight(.bold))
                        .foregroundStyle(canSubmit ? .black : .white.opacity(0.6))
                }
                .buttonStyle(.plain)
                .frame(width: 28, height: 28)
                .background(
                    canSubmit ? Color.white : Color.white.opacity(0.24),
                    in: Circle()
                )
                .disabled(!canSubmit)
                .accessibilityIdentifier("composer.send")
                .accessibilityLabel("Send message")
                #if os(macOS)
                .keyboardShortcut(.return, modifiers: [.command])
                #endif
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.top, 8)
            .padding(.bottom, 6)

            HStack(spacing: 14) {
                Menu {
                    ForEach(sandboxOptions, id: \.self) { option in
                        Button(option) {
                            selectedSandboxMode = option
                        }
                    }
                } label: {
                    Label(selectedSandboxMode, systemImage: "laptopcomputer")
                }
                .menuStyle(.borderlessButton)

                Menu {
                    ForEach(approvalPolicyOptions, id: \.self) { option in
                        Button(option) {
                            selectedApprovalPolicy = option
                        }
                    }
                } label: {
                    Label(selectedApprovalPolicy, systemImage: "shield")
                }
                .menuStyle(.borderlessButton)

                VoiceStatusBadge(label: voiceStateLabel, isActive: voiceInput.isRecording)

                if let voiceCaptureNoticeText {
                    Label(voiceCaptureNoticeText, systemImage: "mic.slash")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .accessibilityIdentifier("composer.voice-notice")
                }

                Spacer()

                HStack(spacing: 5) {
                    Image(systemName: "point.3.connected.trianglepath.dotted")
                    Text(selectedSessionRepoLabel)
                        .lineLimit(1)
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.top, 4)
            .padding(.bottom, 8)
        }
        #endif
    }

    private var emptyState: some View {
        VStack(spacing: 0) {
            Spacer(minLength: emptyStateTopSpacing)

            VStack(spacing: 14) {
                Image(systemName: emptyStateSymbol)
                    .font(.system(size: isCompactPhoneLayout ? 22 : 26, weight: .semibold))
                    .foregroundStyle(.white.opacity(0.88))
                    .frame(width: 48, height: 48)
                    .background(Color.white.opacity(0.08), in: Circle())

                VStack(spacing: 6) {
                    Text(emptyStateTitle)
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.94))
                        .multilineTextAlignment(.center)

                    Text(emptyStateSubtitle)
                        .font(.subheadline)
                        .foregroundStyle(.white.opacity(0.58))
                        .multilineTextAlignment(.center)
                }

                HStack(spacing: 10) {
                    Button(emptyStatePrimaryActionTitle) {
                        emptyStatePrimaryAction()
                    }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("empty.primary-action")

                    Button("Settings") {
                        presentSettings(.general)
                    }
                    .buttonStyle(.bordered)
                    .accessibilityIdentifier("empty.settings")
                }
            }
            .padding(.horizontal, isCompactPhoneLayout ? 20 : 24)
            .padding(.vertical, isCompactPhoneLayout ? 18 : 20)
            .frame(maxWidth: isCompactPhoneLayout ? 360 : 460)
            .background(Color.white.opacity(0.045), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .stroke(Color.white.opacity(0.09), lineWidth: 1)
            )
            .padding(.horizontal, isCompactPhoneLayout ? 20 : 32)

            if !emptyQuickSwitchSessions.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Recent threads")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.68))

                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(emptyQuickSwitchSessions) { session in
                                Button {
                                    store.selectedSessionID = session.id
                                } label: {
                                    VStack(alignment: .leading, spacing: 3) {
                                        Text(sessionSidebarTitle(for: session))
                                            .font(.caption.weight(.semibold))
                                            .foregroundStyle(.white.opacity(0.9))
                                            .lineLimit(1)
                                        Text(relativeAgeLabel(for: session))
                                            .font(.caption2)
                                            .foregroundStyle(.white.opacity(0.54))
                                            .lineLimit(1)
                                    }
                                    .frame(width: isCompactPhoneLayout ? 156 : 180, alignment: .leading)
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 8)
                                    .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                                            .stroke(Color.white.opacity(0.07), lineWidth: 1)
                                    )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
                .frame(maxWidth: isCompactPhoneLayout ? 360 : 460, alignment: .leading)
                .padding(.top, 12)
                .padding(.horizontal, isCompactPhoneLayout ? 20 : 32)
            }

            if showsIPadSplitLayout {
                Text("You can also create threads from the sidebar.")
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.45))
                    .padding(.top, 14)
            }

            Spacer(minLength: emptyStateBottomSpacing)
        }
    }

    private var emptyStateTopSpacing: CGFloat {
        if isCompactPhoneLayout {
            return 20
        }

        if showsIPadSplitLayout {
            return 32
        }

        return 44
    }

    private var emptyStateBottomSpacing: CGFloat {
        if isCompactPhoneLayout {
            return 88
        }

        if showsIPadSplitLayout {
            return 180
        }

        return 120
    }

    private var emptyStateSymbol: String {
        if store.connectionState == .connected {
            return "message"
        }
        return "antenna.radiowaves.left.and.right"
    }

    private var emptyStateTitle: String {
        if store.connectionState == .connected {
            return "Create or choose a thread"
        }
        return "Connect to your local session"
    }

    private var emptyStateSubtitle: String {
        if store.connectionState == .connected {
            return "Start a new thread to mirror Codex events in real time."
        }
        return "Use a local loopback endpoint, then pick a thread to begin."
    }

    private var emptyStatePrimaryActionTitle: String {
        if store.connectionState == .connected {
            return "New thread"
        }
        return "Connect"
    }

    private func emptyStatePrimaryAction() {
        Task {
            if store.connectionState == .connected {
                await store.createSession(cwd: nil)
            } else {
                await store.connect()
            }
        }
    }

    private var canSubmit: Bool {
        canSendTurns && !composerDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var filteredSlashCommands: [ComposerSlashCommand] {
        ComposerSlashCommandCatalog.filteredCoreSet(query: slashCommandQuery)
    }

    private var selectedSessionRootPath: String? {
        store.selectedSession?.cwd.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var trailingMentionMatch: ComposerContextMentionMatch? {
        ComposerContextReferenceFormatter.trailingMentionMatch(in: composerDraft)
    }

    private var inlineContextQuery: String {
        trailingMentionMatch?.query ?? ""
    }

    private var filteredInlineContextPaths: [String] {
        filteredContextPaths(query: inlineContextQuery)
    }

    private var displayedInlineContextPaths: [String] {
        Array(filteredInlineContextPaths.prefix(6))
    }

    private var filteredContextPickerPaths: [String] {
        filteredContextPaths(query: contextPickerQuery)
    }

    private var showsInlineContextSuggestions: Bool {
        composerIsFocused
            && trailingMentionMatch != nil
            && !showContextPicker
            && !displayedInlineContextPaths.isEmpty
    }

    private struct ComposerActivityBadge {
        let label: String
        let icon: String
        let tint: Color
    }

    private var composerActivityBadge: ComposerActivityBadge? {
        for item in store.selectedSessionItems.reversed() {
            guard item.type == "core_event",
                  let payloadType = item.event?.payload?.typeHint
            else {
                continue
            }

            if payloadType == "agent_message" || payloadType == "user_message" || payloadType == "turn_aborted" {
                return nil
            }

            if payloadType == "agent_reasoning" || payloadType == "agent_reasoning_section_break" || payloadType == "task_started" {
                return ComposerActivityBadge(label: "Thinking…", icon: "brain.head.profile", tint: Color.yellow.opacity(0.92))
            }

            if payloadType == "exec_command_begin" || payloadType == "exec_command_output_delta" {
                return ComposerActivityBadge(label: "Executing…", icon: "terminal", tint: Color.green.opacity(0.9))
            }

            if payloadType == "turn_diff" || payloadType == "patch_apply_end" {
                return ComposerActivityBadge(label: "Diffing…", icon: "doc.text.magnifyingglass", tint: Color.cyan.opacity(0.9))
            }

            if payloadType == "background_event" {
                let message = item.body.lowercased()
                if message.contains("browser") {
                    return ComposerActivityBadge(label: "Browsing…", icon: "globe", tint: Color.blue.opacity(0.9))
                }
                if message.contains("review") {
                    return ComposerActivityBadge(label: "Reviewing…", icon: "checklist", tint: Color.purple.opacity(0.86))
                }
                return ComposerActivityBadge(label: "Working…", icon: "sparkles", tint: Color.teal.opacity(0.9))
            }

            if payloadType == "exec_approval_request" || payloadType == "apply_patch_approval_request" {
                return ComposerActivityBadge(label: "Awaiting approval", icon: "hand.raised", tint: Color.orange.opacity(0.9))
            }
        }

        return nil
    }

    private func ensureContextIndexLoaded(forceReload: Bool = false) {
        guard let rootPath = selectedSessionRootPath,
              !rootPath.isEmpty
        else {
            indexedContextRootPath = nil
            indexedContextFilePaths = []
            contextIndexLoading = false
            return
        }

        if !forceReload,
           indexedContextRootPath == rootPath,
           !indexedContextFilePaths.isEmpty {
            return
        }

        if contextIndexLoading {
            return
        }

        contextIndexLoading = true
        Task.detached(priority: .utility) {
            let indexed = buildContextFileIndex(rootPath: rootPath)
            await MainActor.run {
                contextIndexLoading = false
                guard selectedSessionRootPath == rootPath else {
                    return
                }
                indexedContextRootPath = rootPath
                indexedContextFilePaths = indexed
            }
        }
    }

    private func filteredContextPaths(query: String) -> [String] {
        ComposerContextPathCatalog.filteredPaths(
            from: indexedContextFilePaths,
            query: query,
            limit: 30
        )
    }

    private func insertContextReference(path: String) {
        composerDraft = ComposerContextReferenceFormatter.insertReference(
            into: composerDraft,
            path: path,
            mentionMatch: trailingMentionMatch
        )
        selectedInlineContextPath = nil
        contextPickerQuery = ""
        showContextPicker = false
        focusComposerEditor(forceActivateApp: true)
    }

    private func syncInlineContextSelection() {
        guard showsInlineContextSuggestions else {
            selectedInlineContextPath = nil
            return
        }

        guard let selectedInlineContextPath,
              displayedInlineContextPaths.contains(selectedInlineContextPath)
        else {
            self.selectedInlineContextPath = displayedInlineContextPaths.first
            return
        }
    }

    private func moveInlineContextSelection(delta: Int) {
        guard !displayedInlineContextPaths.isEmpty else {
            return
        }

        let fallbackIndex = delta >= 0 ? 0 : max(0, displayedInlineContextPaths.count - 1)
        let currentIndex = selectedInlineContextPath.flatMap { displayedInlineContextPaths.firstIndex(of: $0) } ?? fallbackIndex
        let nextIndex = max(0, min(displayedInlineContextPaths.count - 1, currentIndex + delta))
        selectedInlineContextPath = displayedInlineContextPaths[nextIndex]
    }

    private func acceptInlineContextSelection() {
        guard let path = selectedInlineContextPath ?? displayedInlineContextPaths.first else {
            return
        }

        insertContextReference(path: path)
    }

    private func handleComposerKeyCommand(_ command: ComposerEditorKeyCommand) -> Bool {
        guard showsInlineContextSuggestions else {
            return false
        }

        switch command {
        case .acceptInlineContextSelection:
            acceptInlineContextSelection()
            return true
        case .moveInlineContextSelectionUp:
            moveInlineContextSelection(delta: -1)
            return true
        case .moveInlineContextSelectionDown:
            moveInlineContextSelection(delta: 1)
            return true
        }
    }

    private func handleSlashCommandSelection(_ command: ComposerSlashCommand) {
        showSlashCommandLauncher = false
        slashCommandQuery = ""

        switch command.actionID {
        case .insertCommand(let commandText):
            appendComposerCommand(commandText)
        case .newThread:
            Task {
                await store.createSession(cwd: nil)
            }
        case .refreshThreads:
            Task {
                await store.refreshSessions()
            }
        case .openSettings:
            presentSettings(.general)
        case .openContextPicker:
            ensureContextIndexLoaded()
            showContextPicker = true
        }
    }

    private func appendComposerCommand(_ commandText: String) {
        let trimmed = commandText.trimmingCharacters(in: .newlines)
        guard !trimmed.isEmpty else {
            return
        }

        if composerDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            composerDraft = trimmed
        } else if composerDraft.hasSuffix("\n") {
            composerDraft += trimmed
        } else {
            composerDraft += "\n\(trimmed)"
        }

        focusComposerEditor(forceActivateApp: true)
    }

    private func handleRequestUserInputResponse(
        item: SessionStreamItem,
        answersByQuestionID: [String: [String]]
    ) {
        guard let request = item.requestUserInputPrompt else {
            return
        }

        Task {
            await store.submitRequestUserInput(
                sessionId: item.sessionId,
                turnId: request.turnId,
                answersByQuestionID: answersByQuestionID
            )
        }
    }

    private func presentSettings(_ category: SettingsCategory) {
        settingsCategory = category
        showSettings = true
    }

    private func composerPrimaryAction() {
        Task {
            if store.connectionState != .connected {
                await store.connect()
                #if os(macOS)
                focusComposerEditor(forceActivateApp: true)
                #endif
                return
            }

            if store.selectedSession == nil {
                await store.createSession(cwd: nil)
            }

            #if os(macOS)
            focusComposerEditor(forceActivateApp: true)
            #endif
        }
    }

    private func focusComposerEditor(forceActivateApp: Bool) {
        composerIsFocused = true

        #if os(macOS)
        guard forceActivateApp else {
            return
        }

        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        NSApp.keyWindow?.makeKeyAndOrderFront(nil)
        #endif
    }

    private func copyLastAssistantResponseToPasteboard() {
        #if os(iOS)
        guard let text = lastAssistantResponseText else {
            return
        }
        UIPasteboard.general.string = text
        #endif
    }

    private func submitComposerAction(text overrideText: String? = nil) {
        if voiceInput.isRecording {
            stopVoiceCapture(shouldSubmit: false, clearTranscript: true)
        }

        let text = (overrideText ?? composerDraft).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            return
        }

        composerDraft = ""
        Task {
            await store.submitComposer(text: text)
        }
    }

    private func interruptTurnAction() {
        if voiceInput.isRecording {
            stopVoiceCapture(shouldSubmit: false, clearTranscript: true)
        }
        voiceOutput.stop()

        Task {
            await store.interruptTurn()
        }
    }

    private var composerEditorHeight: CGFloat {
        #if os(macOS)
        let minHeight: CGFloat = 30
        let maxHeight: CGFloat = 120
        return min(maxHeight, max(minHeight, composerMeasuredHeight))
        #else
        let text = composerDraft
        guard !text.isEmpty else {
            return isCompactPhoneLayout ? 36 : 48
        }

        let newlineCount = text.reduce(into: 1) { count, character in
            if character == "\n" {
                count += 1
            }
        }
        let lineCount = max(1, newlineCount)
        let lineHeight: CGFloat = 20
        let verticalPadding: CGFloat = isCompactPhoneLayout ? 14 : 18
        let height = CGFloat(lineCount) * lineHeight + verticalPadding
        let minHeight: CGFloat = isCompactPhoneLayout ? 36 : 48
        let maxHeight: CGFloat = isCompactPhoneLayout ? 140 : 176
        return min(maxHeight, max(minHeight, height))
        #endif
    }

    private var selectedSessionRepoLabel: String {
        guard let selectedSession = store.selectedSession else {
            return "workspace"
        }
        return sessionRepoName(for: selectedSession)
    }

    private func sessionTitle(for session: SessionSummary) -> String {
        let title = sessionSidebarTitle(for: session)
        guard isCompactPhoneLayout else {
            return title
        }

        let maxLength = 56
        guard title.count > maxLength else {
            return title
        }

        let end = title.index(title.startIndex, offsetBy: maxLength)
        return "\(title[..<end])…"
    }

    private func ensureVisibleSelection() {
        if let selectedSession = store.selectedSession,
           !isHiddenSession(selectedSession) {
            return
        }

        store.selectedSessionID = visibleSessions.last?.id
    }

    private func isHiddenSession(_ session: SessionSummary) -> Bool {
        SessionVisibility.isHidden(session)
    }

    private func linkedRepoNameForHiddenSession(_ session: SessionSummary) -> String {
        let url = URL(fileURLWithPath: session.cwd)
        let components = url.pathComponents
        guard let branchIndex = components.firstIndex(of: "branches"),
              branchIndex > 0
        else {
            return sessionRepoName(for: session)
        }

        let repo = components[branchIndex - 1]
        return repo.isEmpty ? sessionRepoName(for: session) : repo
    }

    private func sessionDisplayTitle(for session: SessionSummary) -> String {
        if let threadTitle = sessionThreadTitle(for: session),
           !isLowSignalThreadTitle(threadTitle) {
            return threadTitle
        }

        if let summaryTitle = sessionSummaryTitle(session),
           !isLowSignalThreadTitle(summaryTitle) {
            return summaryTitle
        }

        return sessionRepoName(for: session)
    }

    private func sessionSummaryTitle(_ session: SessionSummary) -> String? {
        guard let title = session.title,
              let cleaned = cleanThreadTitle(title)
        else {
            return nil
        }

        return cleaned
    }

    private func sessionRepoName(for session: SessionSummary) -> String {
        sessionRailRepoName(for: session)
    }

    private func sessionThreadTitle(for session: SessionSummary) -> String? {
        guard let items = store.itemsBySession[session.id],
              !items.isEmpty
        else {
            return nil
        }

        if let updatedName = items.reversed().compactMap(\.threadNameUpdate).first {
            return cleanThreadTitle(updatedName)
        }

        let userTexts = items
            .compactMap(\.userMessageText)
            .filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

        if let latestMeaningful = userTexts
            .reversed()
            .compactMap(cleanThreadTitle)
            .first(where: { !isLowSignalThreadTitle($0) }) {
            return latestMeaningful
        }

        if let firstMeaningful = userTexts
            .compactMap(cleanThreadTitle)
            .first(where: { !isLowSignalThreadTitle($0) }) {
            return firstMeaningful
        }

        if let fallbackUserTitle = userTexts
            .reversed()
            .compactMap(cleanThreadTitle)
            .first {
            return fallbackUserTitle
        }

        if let latestUserText = items
            .reversed()
            .first(where: { $0.userMessageText != nil && !$0.isImagePlaceholderUserMessage })?
            .userMessageText,
           let cleanedTitle = cleanThreadTitle(latestUserText) {
            return cleanedTitle
        }

        if let diffTitle = inferDiffTitle(from: items) {
            return cleanThreadTitle(diffTitle)
        }

        return nil
    }

    private func inferDiffTitle(from items: [SessionStreamItem]) -> String? {
        for item in items.reversed() {
            if let diff = item.turnDiffText,
               let path = threadPrimaryDiffPath(in: diff) {
                let file = URL(fileURLWithPath: path).lastPathComponent
                if !file.isEmpty {
                    return "Edit \(file)"
                }
            }

            if item.isPatchApplyEndEvent {
                let segments = item.body.components(separatedBy: "·").map {
                    $0.trimmingCharacters(in: .whitespacesAndNewlines)
                }
                if let candidate = segments.last,
                   candidate.lowercased().hasSuffix(".swift") ||
                   candidate.lowercased().hasSuffix(".rs") ||
                   candidate.lowercased().hasSuffix(".ts") ||
                   candidate.lowercased().hasSuffix(".js") {
                    return "Edit \(candidate)"
                }
            }
        }

        return nil
    }

    private func threadPrimaryDiffPath(in diff: String) -> String? {
        for line in diff.components(separatedBy: "\n") {
            guard line.hasPrefix("diff --git ") else {
                continue
            }

            let parts = line.split(separator: " ", omittingEmptySubsequences: true)
            guard parts.count >= 4 else {
                continue
            }

            var path = String(parts[3])
            if path.hasPrefix("b/") {
                path.removeFirst(2)
            }
            return path
        }

        return nil
    }

    private func cleanThreadTitle(_ raw: String) -> String? {
        var title = raw
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: "\n", with: " ")
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)

        guard !title.isEmpty else {
            return nil
        }

        if title.hasPrefix("[image:") {
            return nil
        }

        if title.lowercased().contains("[running in read-only mode") {
            return nil
        }

        if title.lowercased().hasPrefix("context: repo:") {
            let pathPart = title.dropFirst("Context: Repo:".count).trimmingCharacters(in: .whitespaces)
            let repo = URL(fileURLWithPath: pathPart).lastPathComponent
            if !repo.isEmpty {
                title = "Context: \(repo)"
            }
        }

        if title.lowercased().hasPrefix("review "),
           let filename = firstFilenameMentioned(in: title) {
            title = "Review \(filename)"
        }

        let maxLength = 120
        if title.count > maxLength {
            let end = title.index(title.startIndex, offsetBy: maxLength)
            title = "\(title[..<end])…"
        }

        return title
    }

    private func isLowSignalThreadTitle(_ value: String) -> Bool {
        let normalized = value
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()

        guard !normalized.isEmpty else {
            return true
        }

        let lowSignalPhrases: Set<String> = [
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

        if normalized.contains("every code harness") {
            return true
        }

        if normalized.contains("session is not from me") {
            return true
        }

        return lowSignalPhrases.contains(normalized)
    }

    private func firstFilenameMentioned(in value: String) -> String? {
        let separators = CharacterSet.whitespacesAndNewlines.union(.punctuationCharacters)
        let tokens = value.components(separatedBy: separators).filter { !$0.isEmpty }
        let supportedSuffixes = [".swift", ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".kt"]

        for token in tokens {
            let lowercased = token.lowercased()
            guard supportedSuffixes.contains(where: { lowercased.hasSuffix($0) }) else {
                continue
            }

            let filename = URL(fileURLWithPath: token).lastPathComponent
            if !filename.isEmpty {
                return filename
            }
        }

        return nil
    }

    private func sessionDisplaySubtitle(for session: SessionSummary) -> String {
        let model = session.model.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalized = model.lowercased()

        if normalized.hasPrefix("unknown model") || normalized == "local runtime" || normalized == "local" {
            return "Local runtime"
        }

        return formatModelName(model)
    }

    private func sessionSidebarSubtitle(for session: SessionSummary) -> String? {
        let model = session.model.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalized = model.lowercased()
        if normalized.hasPrefix("unknown model") || normalized == "local runtime" || normalized == "local" {
            return nil
        }
        return formatModelName(model)
    }

    private func sessionSidebarTitle(for session: SessionSummary) -> String {
        let base = sessionDisplayTitle(for: session)
        let duplicateCount = sessionTitleCounts[base, default: 0]

        guard duplicateCount > 1 else {
            let repo = sessionRepoName(for: session)
            let repoCount = sessionCountByRepoName[repo, default: 0]
            if base.caseInsensitiveCompare(repo) == .orderedSame,
               repoCount > 1 {
                return "\(base) · \(sessionShortID(for: session))"
            }
            return base
        }

        return "\(base) · \(sessionShortID(for: session))"
    }

    private func sessionShortID(for session: SessionSummary) -> String {
        String(session.id.uuidString.prefix(8)).uppercased()
    }

    private func transcriptWidthCap(for item: SessionStreamItem) -> CGFloat {
        let widthDelta = selectedTranscriptDensity.widthDelta
        #if os(iOS)
        let compactCap: CGFloat = (isCompactPhoneLayout ? 318 : 350) + widthDelta
        let toolCap: CGFloat = (isCompactPhoneLayout ? 330 : 360) + widthDelta

        if item.isPatchApplyEndEvent || item.isTokenCountEvent || item.isBackgroundEvent {
            return toolCap
        }

        if item.prefersTrailingBubble {
            let estimated = CGFloat(item.body.count) * 5.4 + 64
            return min(isCompactPhoneLayout ? 252 : 280, max(120, estimated))
        }

        switch item.cardStyle {
        case .tool, .approval:
            return toolCap
        case .reasoning, .composer, .system, .defaultStyle, .assistant, .user:
            return compactCap
        }
        #else
        if item.isPatchApplyEndEvent {
            return 620 + widthDelta
        }

        if item.isTokenCountEvent {
            return 760 + widthDelta
        }

        if item.isBackgroundEvent {
            return 760 + widthDelta
        }

        if item.prefersTrailingBubble {
            let estimated = CGFloat(item.body.count) * 6.4 + 76
            return min(520 + widthDelta, max(170, estimated))
        }

        switch item.cardStyle {
        case .tool, .approval:
            return 700 + widthDelta
        case .reasoning, .composer, .system, .defaultStyle, .assistant, .user:
            return 620 + widthDelta
        }
        #endif
    }

    private func transcriptMinWidth(for item: SessionStreamItem) -> CGFloat {
        #if os(iOS)
        return 0
        #else
        if item.isPatchApplyEndEvent {
            return 420
        }

        if item.isTokenCountEvent {
            return 520
        }

        if item.isBackgroundEvent {
            return 520
        }

        return 0
        #endif
    }

    private func sessionSubtitleLine(for session: SessionSummary) -> String {
        let subtitle = topBarSubtitle(for: session)
        let date = absoluteDateLabel(for: session)

        return "\(subtitle) · \(date)"
    }

    private func topBarSubtitle(for session: SessionSummary) -> String {
        if store.selectedSessionID == session.id,
           let inferred = store.selectedSessionItems
            .reversed()
            .compactMap(\.tokenCountRequestedModel)
            .first {
            return formatModelName(inferred)
        }

        return sessionDisplaySubtitle(for: session)
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

    private func relativeAgeLabel(for session: SessionSummary) -> String {
        let date = sessionActivityDate(for: session)
        let now = Date()
        if date > now || now.timeIntervalSince(date) < 60 {
            return "now"
        }
        return Self.relativeFormatter.localizedString(for: date, relativeTo: now)
    }

    private func absoluteDateLabel(for session: SessionSummary) -> String {
        let date = sessionActivityDate(for: session)
        let calendar = Calendar.current
        if calendar.isDateInToday(date) {
            return Self.timeOnlyFormatter.string(from: date)
        }

        if calendar.isDate(date, equalTo: Date(), toGranularity: .year) {
            return Self.shortDateFormatter.string(from: date)
        }

        return Self.longDateFormatter.string(from: date)
    }

    private func sessionActivityDate(for session: SessionSummary) -> Date {
        Date(timeIntervalSince1970: TimeInterval(sessionActivityUnixMs(session)) / 1_000)
    }

    private func sessionActivityUnixMs(_ session: SessionSummary) -> UInt64 {
        sessionRailActivityUnixMs(session)
    }

    private var ideOpenFailureIsPresented: Binding<Bool> {
        Binding(
            get: { ideOpenFailureMessage != nil },
            set: { isPresented in
                if !isPresented {
                    ideOpenFailureMessage = nil
                }
            }
        )
    }

    private func reportIDEOpenFailure(_ message: String) {
        ideOpenFailureMessage = message
    }

    private func normalizeWorkflowSettings() {
        selectedModel = WorkflowSettings.normalizedSelection(
            current: selectedModel,
            options: modelOptions,
            fallback: "GPT-5.3-Codex"
        )
        selectedReasoningLevel = WorkflowSettings.normalizedSelection(
            current: selectedReasoningLevel,
            options: reasoningOptions,
            fallback: "High"
        )
        selectedSandboxMode = WorkflowSettings.normalizedSelection(
            current: selectedSandboxMode,
            options: sandboxOptions,
            fallback: "Local"
        )
        selectedApprovalPolicy = WorkflowSettings.normalizedSelection(
            current: selectedApprovalPolicy,
            options: approvalPolicyOptions,
            fallback: "On request"
        )
    }

    private func scrollTranscriptToBottom(proxy: ScrollViewProxy, animated: Bool) {
        let action = {
            proxy.scrollTo(transcriptBottomAnchor, anchor: .bottom)
        }

        if animated {
            withAnimation(.easeOut(duration: 0.2)) {
                action()
            }
        } else {
            action()
        }
    }

    private func openSelectedWorkspace() {
        guard let session = store.selectedSession else {
            return
        }

        let destination = OpenDestination(rawValue: openDestinationRaw) ?? .finder
        let url = URL(fileURLWithPath: session.cwd)

        #if os(macOS)
        switch destination {
        case .finder:
            NSWorkspace.shared.open(url)
        case .editor:
            _ = openFileURLInSelectedIDE(
                url,
                sessionID: session.id,
                line: nil,
                column: nil,
                fallbackToDefaultApp: true
            )
        }
        #else
        switch destination {
        case .finder, .editor:
            openURL(url)
        }
        #endif
    }

    private func handleOpenURL(_ url: URL) -> OpenURLAction.Result {
        guard url.scheme == "code-native-file" else {
            return .systemAction(url)
        }

        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false) else {
            return .discarded
        }

        let values = (components.queryItems ?? []).reduce(into: [String: String]()) { result, item in
            result[item.name] = item.value ?? ""
        }
        guard let rawPath = values["path"],
              !rawPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else {
            return .discarded
        }

        let sessionID = values["session"].flatMap(UUID.init(uuidString:))
        let line = values["line"].flatMap(Int.init)
        let column = values["column"].flatMap(Int.init)
        let resolvedURL = resolvedFileURL(path: rawPath, preferredSessionID: sessionID)

        #if os(macOS)
        _ = openFileURLInSelectedIDE(
            resolvedURL,
            sessionID: sessionID,
            line: line,
            column: column,
            fallbackToDefaultApp: true
        )
        #else
        openURL(resolvedURL)
        #endif

        return .handled
    }

    private func resolvedFileURL(path: String, preferredSessionID: UUID?) -> URL {
        let raw = path.trimmingCharacters(in: .whitespacesAndNewlines)

        if raw.hasPrefix("/") {
            return URL(fileURLWithPath: raw).standardizedFileURL
        }

        let normalized = normalizedRelativePath(raw)
        if let session = resolvedSession(for: preferredSessionID) {
            let candidate = URL(fileURLWithPath: session.cwd)
                .appendingPathComponent(normalized)
                .standardizedFileURL
            if FileManager.default.fileExists(atPath: candidate.path) {
                return candidate
            }
        }

        return URL(fileURLWithPath: normalized).standardizedFileURL
    }

    private func normalizedRelativePath(_ raw: String) -> String {
        if raw.hasPrefix("a/") || raw.hasPrefix("b/") {
            return String(raw.dropFirst(2))
        }
        return raw
    }

    private func resolvedSession(for preferredSessionID: UUID?) -> SessionSummary? {
        if let preferredSessionID,
           let session = store.sessions.first(where: { $0.id == preferredSessionID }) {
            return session
        }

        if let selected = store.selectedSession {
            return selected
        }

        return store.sessions.last
    }

    private func selectedIDE(for sessionID: UUID) -> SessionIDESelection {
        SessionIDEPreferences.selectedIDE(
            for: sessionID,
            rawMap: sessionIDEMapRaw,
            rawDefaultIDE: defaultSessionIDERaw,
            available: availableSessionIDEs()
        )
    }

    private func setSessionIDE(_ ide: SessionIDESelection, for sessionID: UUID) {
        sessionIDEMapRaw = SessionIDEPreferences.storing(
            ide: ide,
            for: sessionID,
            rawMap: sessionIDEMapRaw
        )
    }

    private func availableSessionIDEs() -> [SessionIDESelection] {
        #if os(macOS)
        IDEAvailability.availableSelections()
        #else
        [.systemDefault]
        #endif
    }

    private func pruneSessionIDEPreferences() {
        let validSessionIDs = Set(store.sessions.map(\.id))
        let available = availableSessionIDEs()
        sessionIDEMapRaw = SessionIDEPreferences.pruned(
            rawMap: sessionIDEMapRaw,
            validSessionIDs: validSessionIDs,
            available: available
        )
        defaultSessionIDERaw = SessionIDEPreferences.normalizedDefaultIDE(
            rawDefaultIDE: defaultSessionIDERaw,
            available: available
        )
    }

    #if os(macOS)
    private func openFileURLInSelectedIDE(
        _ fileURL: URL,
        sessionID: UUID?,
        line: Int?,
        column: Int?,
        fallbackToDefaultApp: Bool
    ) -> Bool {
        let ide = sessionID.map(selectedIDE(for:)) ?? .systemDefault

        let locationSuffix: String
        if let line {
            if let column {
                locationSuffix = ":\(line):\(column)"
            } else {
                locationSuffix = ":\(line)"
            }
        } else {
            locationSuffix = ""
        }
        let targetDescription = "\(fileURL.path)\(locationSuffix)"

        if ide != .systemDefault,
           !ide.isInstalled() {
            let message = "\(ide.label) is not installed. Choose another IDE for this session."
            if fallbackToDefaultApp,
               NSWorkspace.shared.open(fileURL) {
                reportIDEOpenFailure("\(message) Opened \(targetDescription) in the default app instead.")
                return true
            }

            reportIDEOpenFailure("\(message) Unable to open \(targetDescription).")
            return false
        }

        if let deepLink = ide.deepLinkURL(for: fileURL, line: line, column: column),
           NSWorkspace.shared.open(deepLink) {
            return true
        }

        if let appURL = ide.resolvedApplicationURL() {
            NSWorkspace.shared.open(
                [fileURL],
                withApplicationAt: appURL,
                configuration: NSWorkspace.OpenConfiguration()
            ) { _, error in
                guard let error else {
                    return
                }

                if fallbackToDefaultApp,
                   NSWorkspace.shared.open(fileURL) {
                    Task { @MainActor in
                        reportIDEOpenFailure("Could not open \(targetDescription) in \(ide.label): \(error.localizedDescription). Opened in the default app instead.")
                    }
                    return
                }

                Task { @MainActor in
                    reportIDEOpenFailure("Could not open \(targetDescription) in \(ide.label): \(error.localizedDescription)")
                }
            }
            return true
        }

        if ide != .systemDefault {
            let message = "No launchable app was found for \(ide.label)."
            if fallbackToDefaultApp,
               NSWorkspace.shared.open(fileURL) {
                reportIDEOpenFailure("\(message) Opened \(targetDescription) in the default app instead.")
                return true
            }

            reportIDEOpenFailure("\(message) Unable to open \(targetDescription).")
            return false
        }

        if fallbackToDefaultApp {
            let opened = NSWorkspace.shared.open(fileURL)
            if !opened {
                reportIDEOpenFailure("Unable to open \(targetDescription) in the default app.")
            }
            return opened
        }

        return false
    }
    #endif

    private func startVoiceCapture() {
        if let voiceCaptureGuardReason {
            voiceInteractionNotice = voiceCaptureGuardReason.helperLabel
            return
        }

        voiceOutput.stop()
        voiceInteractionNotice = nil
        Task {
            await voiceInput.startRecording { text, _ in
                Task { @MainActor in
                    composerDraft = text
                }
            }
        }
    }

    private func stopVoiceCapture(shouldSubmit: Bool, clearTranscript: Bool = false) {
        let wasRecording = voiceInput.isRecording
        let guardReasonAtStop = voiceCaptureGuardReason
        let finalText = voiceInput.stopRecording(clearTranscript: clearTranscript)
        let normalized = finalText.trimmingCharacters(in: .whitespacesAndNewlines)

        if wasRecording {
            composerDraft = finalText
        }

        if !wasRecording,
           !clearTranscript {
            return
        }

        let shouldAutoSubmit = VoiceInteractionPolicy.shouldAutoSubmitCapture(
            autoSubmitEnabled: shouldSubmit && autoSubmitVoice,
            normalizedDraft: normalized,
            guardReason: guardReasonAtStop
        )

        if shouldAutoSubmit {
            composerDraft = ""
            voiceInput.clearTranscript()
            voiceInteractionNotice = nil
            Task {
                await store.submitComposer(text: finalText)
            }
            return
        }

        if shouldSubmit,
           autoSubmitVoice,
           !normalized.isEmpty,
           let guardReasonAtStop {
            voiceInteractionNotice = guardReasonAtStop.helperLabel
        } else if clearTranscript {
            voiceInteractionNotice = guardReasonAtStop?.helperLabel
        }
    }

    private func enforceVoiceCapturePolicy(clearTranscript: Bool) {
        let guardReason = voiceCaptureGuardReason
        if let guardReason,
           !voiceInput.isRecording {
            voiceInteractionNotice = guardReason.helperLabel
        } else if guardReason == nil,
                  !voiceInput.isRecording {
            voiceInteractionNotice = nil
        }

        if VoiceInteractionPolicy.shouldStopActiveCapture(
            isRecording: voiceInput.isRecording,
            guardReason: guardReason
        ) {
            stopVoiceCapture(shouldSubmit: false, clearTranscript: clearTranscript)
            voiceInteractionNotice = guardReason?.helperLabel
        }
    }

    private func handleVoiceToggleTap() {
        if voiceInput.isRecording {
            stopVoiceCapture(shouldSubmit: true)
            return
        }

        startVoiceCapture()
    }

    private func handleApproval(item: SessionStreamItem, decision: ApprovalDecisionChoice) {
        guard let approvalRequest = item.approvalRequest else {
            return
        }

        if voiceInput.isRecording {
            stopVoiceCapture(shouldSubmit: false, clearTranscript: true)
        }

        Task {
            await store.submitApproval(
                sessionId: item.sessionId,
                callId: approvalRequest.callId,
                type: approvalRequest.type,
                decision: decision
            )
        }
    }

    private func handleAssistantSpeech() {
        guard autoSpeakAssistant,
              !voiceInput.isRecording,
              store.connectionState == .connected,
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

private enum TranscriptDensity: String, CaseIterable, Identifiable {
    case compact
    case comfortable
    case roomy

    var id: String { rawValue }

    var label: String {
        switch self {
        case .compact:
            return "Compact"
        case .comfortable:
            return "Comfortable"
        case .roomy:
            return "Roomy"
        }
    }

    var rowSpacing: CGFloat {
        switch self {
        case .compact:
            return 10
        case .comfortable:
            return 12
        case .roomy:
            return 14
        }
    }

    var horizontalPadding: CGFloat {
        switch self {
        case .compact:
            return 10
        case .comfortable:
            return 12
        case .roomy:
            return 14
        }
    }

    var bubbleGutter: CGFloat {
        switch self {
        case .compact:
            return 8
        case .comfortable:
            return 10
        case .roomy:
            return 12
        }
    }

    var widthDelta: CGFloat {
        switch self {
        case .compact:
            return -14
        case .comfortable:
            return 0
        case .roomy:
            return 24
        }
    }

    var lineSpacing: CGFloat {
        switch self {
        case .compact:
            return 2
        case .comfortable:
            return 3
        case .roomy:
            return 4
        }
    }

    var cardHorizontalPadding: CGFloat {
        switch self {
        case .compact:
            return 12
        case .comfortable:
            return 14
        case .roomy:
            return 16
        }
    }

    var cardVerticalPadding: CGFloat {
        switch self {
        case .compact:
            return 10
        case .comfortable:
            return 12
        case .roomy:
            return 14
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

enum SessionIDESelection: String, CaseIterable, Identifiable {
    case systemDefault
    case cursor
    case vsCode
    case zed
    case intelliJ
    case pyCharm
    case xcode

    var id: String { rawValue }

    var label: String {
        switch self {
        case .systemDefault:
            return "System IDE"
        case .cursor:
            return "Cursor"
        case .vsCode:
            return "VS Code"
        case .zed:
            return "Zed"
        case .intelliJ:
            return "IntelliJ"
        case .pyCharm:
            return "PyCharm"
        case .xcode:
            return "Xcode"
        }
    }

    #if os(macOS)
    var bundleIdentifiers: [String] {
        switch self {
        case .systemDefault:
            return []
        case .cursor:
            return ["com.todesktop.230313mzl4w4u92", "com.cursor.app"]
        case .vsCode:
            return ["com.microsoft.VSCode", "com.microsoft.VSCodeInsiders"]
        case .zed:
            return ["dev.zed.Zed"]
        case .intelliJ:
            return ["com.jetbrains.intellij", "com.jetbrains.intellij.ce", "com.jetbrains.intellij-EAP"]
        case .pyCharm:
            return ["com.jetbrains.pycharm", "com.jetbrains.pycharm.ce", "com.jetbrains.pycharm-EAP"]
        case .xcode:
            return ["com.apple.dt.Xcode"]
        }
    }

    var applicationNames: [String] {
        switch self {
        case .systemDefault:
            return []
        case .cursor:
            return ["Cursor"]
        case .vsCode:
            return ["Visual Studio Code", "Visual Studio Code - Insiders"]
        case .zed:
            return ["Zed"]
        case .intelliJ:
            return ["IntelliJ IDEA", "IntelliJ IDEA CE", "IntelliJ IDEA Ultimate", "IntelliJ IDEA EAP"]
        case .pyCharm:
            return ["PyCharm", "PyCharm CE", "PyCharm Professional", "PyCharm EAP"]
        case .xcode:
            return ["Xcode"]
        }
    }

    func installedBundleIdentifier(
        using resolver: IDEApplicationResolving = LiveIDEApplicationResolver.shared
    ) -> String? {
        for bundleIdentifier in bundleIdentifiers {
            if resolver.urlForApplication(bundleIdentifier: bundleIdentifier) != nil {
                return bundleIdentifier
            }
        }

        return nil
    }

    func resolvedApplicationURL(
        using resolver: IDEApplicationResolving = LiveIDEApplicationResolver.shared
    ) -> URL? {
        for bundleIdentifier in bundleIdentifiers {
            if let url = resolver.urlForApplication(bundleIdentifier: bundleIdentifier) {
                return url
            }
        }

        for appName in applicationNames {
            if let url = resolver.urlForApplication(named: appName) {
                return url
            }
        }

        return nil
    }

    var fileURLScheme: String? {
        switch self {
        case .systemDefault, .xcode:
            return nil
        case .cursor:
            return "cursor"
        case .vsCode:
            return "vscode"
        case .zed:
            return "zed"
        case .intelliJ:
            return "idea"
        case .pyCharm:
            return "pycharm"
        }
    }

    var isAvailable: Bool {
        isInstalled()
    }

    func isInstalled(
        using resolver: IDEApplicationResolving = LiveIDEApplicationResolver.shared
    ) -> Bool {
        if self == .systemDefault {
            return true
        }

        return resolvedApplicationURL(using: resolver) != nil
    }

    func deepLinkURL(for fileURL: URL, line: Int?, column: Int?) -> URL? {
        guard let fileURLScheme else {
            return nil
        }

        if self == .intelliJ || self == .pyCharm {
            var components = URLComponents()
            components.scheme = fileURLScheme
            components.host = "open"

            var queryItems = [URLQueryItem(name: "file", value: fileURL.path)]
            if let line {
                queryItems.append(URLQueryItem(name: "line", value: String(line)))
            }
            if let column {
                queryItems.append(URLQueryItem(name: "column", value: String(column)))
            }

            components.queryItems = queryItems
            return components.url
        }

        let encodedPath = fileURL.path.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? fileURL.path
        var value = "\(fileURLScheme)://file\(encodedPath)"
        if let line {
            value += ":\(line)"
            if let column {
                value += ":\(column)"
            }
        }

        return URL(string: value)
    }
    #else
    var isAvailable: Bool {
        self == .systemDefault
    }
    #endif
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
    let accessibilityID: String
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
        .background(Color.white.opacity(0.035), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .stroke(Color.white.opacity(0.08), lineWidth: 1)
        )
        .accessibilityIdentifier(accessibilityID)
    }
}

private struct ThreadPill: View {
    let title: String
    let subtitle: String?
    let trailingLabel: String
    let accessibilityID: String
    let isSelected: Bool
    let density: ThreadDensity
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 10) {
                Circle()
                    .fill(isSelected ? Color.white.opacity(0.92) : Color.white.opacity(0.16))
                    .frame(width: 4, height: 4)
                VStack(alignment: .leading, spacing: 3) {
                    Text(title)
                        .font(.subheadline.weight(.semibold))
                        .lineLimit(1)
                    if let subtitle,
                       !subtitle.isEmpty {
                        Text(subtitle)
                            .font(.caption)
                            .foregroundStyle(.white.opacity(0.55))
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 6)
                Text(trailingLabel)
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.5))
                    .frame(minWidth: 34, alignment: .trailing)
            }
            .padding(.horizontal, 9)
            .padding(.vertical, density.rowPadding + 1)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(isSelected ? .white : .white.opacity(0.87))
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(isSelected ? Color.white.opacity(0.14) : Color.white.opacity(0.025))
        )
        .overlay {
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .stroke(isSelected ? Color.white.opacity(0.16) : Color.white.opacity(0.04), lineWidth: 1)
        }
        .accessibilityIdentifier(accessibilityID)
    }
}

private struct TopBarButton: View {
    let icon: String
    let title: String
    let accessibilityID: String
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
        .accessibilityIdentifier(accessibilityID)
    }
}

private struct TopBarIconButton: View {
    let icon: String
    let accessibilityID: String
    let buttonSize: CGFloat
    let action: () -> Void

    init(icon: String, accessibilityID: String, buttonSize: CGFloat = 34, action: @escaping () -> Void) {
        self.icon = icon
        self.accessibilityID = accessibilityID
        self.buttonSize = buttonSize
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.subheadline.weight(.semibold))
                .frame(width: buttonSize, height: buttonSize)
                .background(Color.white.opacity(0.06), in: Circle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(.white.opacity(0.9))
        .accessibilityIdentifier(accessibilityID)
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
                .accessibilityIdentifier("connection.endpoint")

            HStack(spacing: 8) {
                Button("Connect") {
                    Task {
                        await store.connect()
                    }
                }
                .disabled(store.connectionState != .disconnected)
                .accessibilityIdentifier("connection.connect")

                Button("Disconnect") {
                    store.disconnect()
                }
                .disabled(store.connectionState == .disconnected)
                .accessibilityIdentifier("connection.disconnect")

                Button("Refresh") {
                    Task {
                        await store.refreshSessions()
                    }
                }
                .disabled(store.connectionState != .connected)
                .accessibilityIdentifier("connection.refresh")
            }
            .buttonStyle(.borderedProminent)

            if let error = store.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            } else {
                VStack(alignment: .leading, spacing: 4) {
                    Text(store.selectedSessionRuntimeState.label)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.primary)

                    Text(store.statusLine)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            let telemetry = store.historyPageTelemetry
            if telemetry.requestCount > 0 {
                VStack(alignment: .leading, spacing: 2) {
                    Text(
                        "History pages · req \(telemetry.requestCount) · ok \(telemetry.successCount) · avg \(Int(telemetry.averageLatencyMs))ms · slow \(telemetry.slowPageCount)"
                    )
                    .font(.caption2.monospaced())
                    .foregroundStyle(.secondary)

                    if telemetry.inactiveRetentionPasses > 0 {
                        Text(
                            "Retention trims · passes \(telemetry.inactiveRetentionPasses) · items \(telemetry.inactiveRetentionTrimmedItems)"
                        )
                        .font(.caption2.monospaced())
                        .foregroundStyle(.secondary)
                    }
                }
            }
        }
    }
}

private struct QuickPromptCard: View {
    let prompt: String
    let lineLimit: Int
    let minHeight: CGFloat
    let action: () -> Void

    init(prompt: String, lineLimit: Int = 3, minHeight: CGFloat = 112, action: @escaping () -> Void) {
        self.prompt = prompt
        self.lineLimit = lineLimit
        self.minHeight = minHeight
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 8) {
                Image(systemName: "sparkle")
                    .font(.subheadline)
                    .foregroundStyle(.white.opacity(0.75))
                Text(prompt)
                    .font(.body.weight(.medium))
                    .foregroundStyle(.white.opacity(0.9))
                    .lineLimit(lineLimit)
                    .multilineTextAlignment(.leading)
            }
            .padding(14)
            .frame(maxWidth: .infinity, minHeight: minHeight, alignment: .topLeading)
            .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .stroke(Color.white.opacity(0.08), lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }
}

private enum ComposerEditorKeyCommand {
    case acceptInlineContextSelection
    case moveInlineContextSelectionUp
    case moveInlineContextSelectionDown
}

#if os(macOS)
private struct MacComposerTextView: NSViewRepresentable {
    @Binding var text: String
    var isFocused: Bool
    @Binding var measuredHeight: CGFloat
    let onFocusChange: (Bool) -> Void
    let onSubmit: (String) -> Void
    let onEditorKeyCommand: (ComposerEditorKeyCommand) -> Bool

    func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.drawsBackground = false
        scrollView.hasVerticalScroller = false
        scrollView.hasHorizontalScroller = false
        scrollView.borderType = .noBorder

        let textView = SubmitAwareTextView()
        textView.drawsBackground = false
        textView.backgroundColor = .clear
        textView.font = NSFont.systemFont(ofSize: NSFont.systemFontSize)
        textView.textColor = NSColor.white.withAlphaComponent(0.9)
        textView.insertionPointColor = NSColor.white.withAlphaComponent(0.95)
        textView.isRichText = false
        textView.importsGraphics = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticLinkDetectionEnabled = true
        textView.allowsUndo = true
        textView.textContainerInset = NSSize(width: 0, height: 4)
        textView.delegate = context.coordinator
        textView.onSubmit = { [weak textView, weak coordinator = context.coordinator] in
            coordinator?.cancelPendingSync()
            onSubmit(textView?.string ?? "")
        }
        textView.onEditorKeyCommand = onEditorKeyCommand
        textView.string = text
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.minSize = NSSize(width: 0, height: 24)
        textView.textContainer?.containerSize = NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude)
        textView.textContainer?.widthTracksTextView = true

        scrollView.documentView = textView

        DispatchQueue.main.async {
            measuredHeight = Self.measuredEditorHeight(for: textView)
        }

        return scrollView
    }

    func updateNSView(_ nsView: NSScrollView, context: Context) {
        context.coordinator.parent = self

        guard let textView = nsView.documentView as? SubmitAwareTextView else {
            return
        }

        if textView.string != text {
            context.coordinator.cancelPendingSync()
            textView.string = text
        }

        textView.onSubmit = { [weak textView, weak coordinator = context.coordinator] in
            coordinator?.cancelPendingSync()
            onSubmit(textView?.string ?? "")
        }
        textView.onEditorKeyCommand = onEditorKeyCommand

        if isFocused,
           let window = nsView.window,
           window.firstResponder !== textView {
            window.makeFirstResponder(textView)
        }
    }

    static func measuredEditorHeight(for textView: NSTextView) -> CGFloat {
        guard let layoutManager = textView.layoutManager,
              let textContainer = textView.textContainer
        else {
            return 34
        }

        layoutManager.ensureLayout(for: textContainer)
        let used = layoutManager.usedRect(for: textContainer)
        let verticalInset = textView.textContainerInset.height * 2
        return ceil(max(24, used.height + verticalInset + 2))
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: MacComposerTextView
        private var pendingSync: DispatchWorkItem?

        init(parent: MacComposerTextView) {
            self.parent = parent
        }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else {
                return
            }

            let updated = textView.string
            parent.measuredHeight = MacComposerTextView.measuredEditorHeight(for: textView)

            pendingSync?.cancel()
            let work = DispatchWorkItem { [weak self] in
                guard let self else {
                    return
                }

                if self.parent.text != updated {
                    self.parent.text = updated
                }
            }

            pendingSync = work
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.18, execute: work)
        }

        func cancelPendingSync() {
            pendingSync?.cancel()
            pendingSync = nil
        }

        func textDidBeginEditing(_ notification: Notification) {
            _ = notification
            if !parent.isFocused {
                parent.onFocusChange(true)
            }
        }

        func textDidEndEditing(_ notification: Notification) {
            _ = notification
            if parent.isFocused {
                parent.onFocusChange(false)
            }
        }
    }

    final class SubmitAwareTextView: NSTextView {
        var onSubmit: (() -> Void)?
        var onEditorKeyCommand: ((ComposerEditorKeyCommand) -> Bool)?

        override func keyDown(with event: NSEvent) {
            let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            let hasShift = modifiers.contains(.shift)
            let hasCommand = modifiers.contains(.command)
            let hasOption = modifiers.contains(.option)
            let hasControl = modifiers.contains(.control)

            if !hasShift && !hasCommand && !hasOption && !hasControl {
                if event.keyCode == 48,
                   onEditorKeyCommand?(.acceptInlineContextSelection) == true {
                    return
                }

                if event.keyCode == 126,
                   onEditorKeyCommand?(.moveInlineContextSelectionUp) == true {
                    return
                }

                if event.keyCode == 125,
                   onEditorKeyCommand?(.moveInlineContextSelectionDown) == true {
                    return
                }
            }

            let isReturnKey = event.keyCode == 36 || event.keyCode == 76
            if isReturnKey {
                if hasShift && !hasCommand && !hasOption && !hasControl {
                    insertNewline(nil)
                    return
                }

                if !hasShift && !hasCommand && !hasOption && !hasControl {
                    onSubmit?()
                    return
                }
            }

            super.keyDown(with: event)
        }
    }
}
#endif

private struct AssistantTranscriptLine: View {
    let text: String
    let sessionID: UUID
    let density: TranscriptDensity
    let isActive: Bool
    let onActivate: () -> Void
    private let parsedBlocks: [MarkdownBlock]
    @State private var isHoveringCopyActions = false

    private enum MarkdownBlock: Hashable {
        case paragraph(String)
        case heading(level: Int, text: String)
        case unorderedListItem(level: Int, text: String)
        case orderedListItem(level: Int, number: String, text: String)
        case quote(String)
        case code(language: String?, text: String)
    }

    init(
        text: String,
        sessionID: UUID,
        density: TranscriptDensity = .comfortable,
        isActive: Bool = false,
        onActivate: @escaping () -> Void = {}
    ) {
        self.text = text
        self.sessionID = sessionID
        self.density = density
        self.isActive = isActive
        self.onActivate = onActivate
        self.parsedBlocks = Self.parseMarkdownBlocks(from: text)
    }

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    private var usesCompactPhoneTypography: Bool {
        #if os(iOS)
        return UIDevice.current.userInterfaceIdiom == .phone && horizontalSizeClass == .compact
        #else
        return false
        #endif
    }

    private var canCopyText: Bool {
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var showsCopyActions: Bool {
        guard canCopyText else {
            return false
        }

        #if os(macOS)
        return isActive || isHoveringCopyActions
        #else
        return isActive
        #endif
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(parsedBlocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .paragraph(let value):
                    AssistantInlineMarkdownText(
                        text: value,
                        baseColor: Color.white.opacity(0.93),
                        sessionID: sessionID
                    )
                        .font(usesCompactPhoneTypography ? .callout : .body)
                        .lineSpacing(usesCompactPhoneTypography ? 2 : density.lineSpacing)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)

                case .heading(let level, let value):
                    AssistantInlineMarkdownText(
                        text: value,
                        baseColor: headingColor(for: level),
                        sessionID: sessionID
                    )
                        .font(headingFont(for: level))
                        .lineSpacing(usesCompactPhoneTypography ? 2 : density.lineSpacing)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)

                case .unorderedListItem(let level, let value):
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(unorderedMarker(for: level))
                            .font((usesCompactPhoneTypography ? Font.callout : Font.body).weight(.semibold))
                            .foregroundStyle(unorderedMarkerColor(for: level))
                        AssistantInlineMarkdownText(
                            text: value,
                            baseColor: Color.white.opacity(0.93),
                            sessionID: sessionID
                        )
                            .font(usesCompactPhoneTypography ? .callout : .body)
                            .lineSpacing(usesCompactPhoneTypography ? 2 : density.lineSpacing)
                            .textSelection(.enabled)
                    }
                    .padding(.leading, CGFloat(level * 14))
                    .frame(maxWidth: .infinity, alignment: .leading)

                case .orderedListItem(let level, let number, let value):
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(number).")
                            .font((usesCompactPhoneTypography ? Font.callout : Font.body).monospaced().weight(.semibold))
                            .foregroundStyle(Color.blue.opacity(0.85))
                        AssistantInlineMarkdownText(
                            text: value,
                            baseColor: Color.white.opacity(0.93),
                            sessionID: sessionID
                        )
                            .font(usesCompactPhoneTypography ? .callout : .body)
                            .lineSpacing(usesCompactPhoneTypography ? 2 : density.lineSpacing)
                            .textSelection(.enabled)
                    }
                    .padding(.leading, CGFloat(level * 14))
                    .frame(maxWidth: .infinity, alignment: .leading)

                case .quote(let value):
                    HStack(alignment: .top, spacing: 8) {
                        RoundedRectangle(cornerRadius: 2, style: .continuous)
                            .fill(Color.cyan.opacity(0.45))
                            .frame(width: 3)
                        AssistantInlineMarkdownText(
                            text: value,
                            baseColor: Color.white.opacity(0.88),
                            sessionID: sessionID
                        )
                            .font(usesCompactPhoneTypography ? .callout : .body)
                            .lineSpacing(usesCompactPhoneTypography ? 2 : density.lineSpacing)
                            .textSelection(.enabled)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
                    .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))

                case .code(let language, let value):
                    AssistantCodeBlock(language: language, code: value)
                }
            }
        }
        .padding(.horizontal, usesCompactPhoneTypography ? 2 : 4)
        .padding(.vertical, 2)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(alignment: .bottomTrailing) {
            Button {
                copyTextToPasteboard(text)
            } label: {
                Image(systemName: "doc.on.doc")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.86))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 6)
                    .background(Color.black.opacity(0.40), in: Capsule(style: .continuous))
                    .overlay(
                        Capsule(style: .continuous)
                            .stroke(Color.white.opacity(0.10), lineWidth: 1)
                    )
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Copy message")
            .padding(.trailing, usesCompactPhoneTypography ? 2 : 6)
            .padding(.bottom, 4)
            .opacity(showsCopyActions ? 1 : 0)
            .animation(.easeInOut(duration: 0.16), value: showsCopyActions)
            .allowsHitTesting(showsCopyActions)
        }
        .contentShape(Rectangle())
        .onTapGesture {
            onActivate()
        }
        #if os(macOS)
        .onHover { hovering in
            isHoveringCopyActions = hovering
        }
        #endif
        .contextMenu {
            if canCopyText {
                Button("Copy text") {
                    copyTextToPasteboard(text)
                }
            }
        }
    }

    private func copyTextToPasteboard(_ value: String) {
        #if os(macOS)
        let board = NSPasteboard.general
        board.clearContents()
        board.setString(value, forType: .string)
        #elseif os(iOS)
        UIPasteboard.general.string = value
        #endif
    }

    private func headingFont(for level: Int) -> Font {
        switch level {
        case 1:
            return .title3.weight(.semibold)
        case 2:
            return .headline.weight(.semibold)
        default:
            return .subheadline.weight(.semibold)
        }
    }

    private func headingColor(for level: Int) -> Color {
        switch level {
        case 1:
            return Color.blue.opacity(0.95)
        case 2:
            return Color.blue.opacity(0.9)
        default:
            return Color.blue.opacity(0.82)
        }
    }

    private static func parseMarkdownBlocks(from value: String) -> [MarkdownBlock] {
        let normalized = value.replacingOccurrences(of: "\r\n", with: "\n")
        guard !normalized.isEmpty else {
            return []
        }

        let lines = normalized.components(separatedBy: "\n")
        var blocks: [MarkdownBlock] = []
        var proseBuffer: [String] = []
        var codeBuffer: [String] = []
        var codeLanguage: String?
        var inCodeBlock = false

        func flushProse() {
            guard !proseBuffer.isEmpty else {
                return
            }

            blocks.append(contentsOf: parseProseLines(proseBuffer))
            proseBuffer.removeAll(keepingCapacity: true)
        }

        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            if trimmed.hasPrefix("```") {
                if inCodeBlock {
                    let code = codeBuffer.joined(separator: "\n")
                    blocks.append(.code(language: codeLanguage, text: code))
                    codeBuffer.removeAll(keepingCapacity: true)
                    codeLanguage = nil
                    inCodeBlock = false
                } else {
                    flushProse()
                    let languageHint = String(trimmed.dropFirst(3)).trimmingCharacters(in: .whitespacesAndNewlines)
                    codeLanguage = languageHint.isEmpty ? nil : languageHint
                    inCodeBlock = true
                }
                continue
            }

            if inCodeBlock {
                codeBuffer.append(line)
            } else {
                proseBuffer.append(line)
            }
        }

        if inCodeBlock {
            let code = codeBuffer.joined(separator: "\n")
            blocks.append(.code(language: codeLanguage, text: code))
        }

        flushProse()

        return blocks
    }

    private static func parseProseLines(_ lines: [String]) -> [MarkdownBlock] {
        var blocks: [MarkdownBlock] = []
        var paragraphBuffer: [String] = []

        func flushParagraph() {
            let paragraph = paragraphBuffer
                .joined(separator: "\n")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            guard !paragraph.isEmpty else {
                paragraphBuffer.removeAll(keepingCapacity: true)
                return
            }

            blocks.append(.paragraph(paragraph))
            paragraphBuffer.removeAll(keepingCapacity: true)
        }

        for raw in lines {
            let trimmed = raw.trimmingCharacters(in: .whitespaces)
            let indentSpaces = leadingIndentCount(in: raw)
            let listLevel = max(0, indentSpaces / 2)

            if trimmed.isEmpty {
                flushParagraph()
                continue
            }

            if trimmed.hasPrefix("#") {
                flushParagraph()
                let level = min(3, trimmed.prefix { $0 == "#" }.count)
                let title = trimmed.drop(while: { $0 == "#" || $0 == " " })
                blocks.append(.heading(level: level, text: String(title)))
                continue
            }

            if trimmed.hasPrefix(">") {
                flushParagraph()
                let quote = trimmed.drop(while: { $0 == ">" || $0 == " " })
                blocks.append(.quote(String(quote)))
                continue
            }

            if trimmed.hasPrefix("- ") || trimmed.hasPrefix("* ") {
                flushParagraph()
                let value = trimmed.dropFirst(2).trimmingCharacters(in: .whitespaces)
                blocks.append(.unorderedListItem(level: listLevel, text: value))
                continue
            }

            if let ordered = parseOrderedListItem(from: trimmed) {
                flushParagraph()
                blocks.append(.orderedListItem(level: listLevel, number: ordered.number, text: ordered.text))
                continue
            }

            paragraphBuffer.append(raw)
        }

        flushParagraph()
        return blocks
    }

    private static func leadingIndentCount(in line: String) -> Int {
        var count = 0
        for char in line {
            if char == " " {
                count += 1
                continue
            }
            if char == "\t" {
                count += 4
                continue
            }
            break
        }
        return count
    }

    private static func parseOrderedListItem(from line: String) -> (number: String, text: String)? {
        var digits = ""
        var index = line.startIndex

        while index < line.endIndex,
              line[index].isNumber {
            digits.append(line[index])
            index = line.index(after: index)
        }

        guard !digits.isEmpty,
              index < line.endIndex,
              line[index] == "."
        else {
            return nil
        }

        index = line.index(after: index)
        guard index < line.endIndex,
              line[index] == " "
        else {
            return nil
        }

        let content = line[line.index(after: index)...].trimmingCharacters(in: .whitespaces)
        guard !content.isEmpty else {
            return nil
        }

        return (digits, content)
    }

    private func unorderedMarker(for level: Int) -> String {
        if level == 0 {
            return "-"
        }

        if level == 1 {
            return "•"
        }

        return "◦"
    }

    private func unorderedMarkerColor(for level: Int) -> Color {
        if level == 0 {
            return Color.cyan.opacity(0.92)
        }

        if level == 1 {
            return Color.teal.opacity(0.9)
        }

        return Color.teal.opacity(0.72)
    }
}

private struct AssistantInlineMarkdownText: View {
    private let attributed: AttributedString

    init(text: String, baseColor: Color = Color.white.opacity(0.93), sessionID: UUID?) {
        self.attributed = Self.makeStyledAttributedString(from: text, baseColor: baseColor, sessionID: sessionID)
    }

    var body: some View {
        Text(attributed)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    private struct FileReferenceToken {
        let path: String
        let line: Int?
        let column: Int?
    }

    private static func makeStyledAttributedString(from text: String, baseColor: Color, sessionID: UUID?) -> AttributedString {
        let linkedText = injectFileLinks(into: text, sessionID: sessionID)
        let parsed = try? AttributedString(
            markdown: linkedText,
            options: AttributedString.MarkdownParsingOptions(
                interpretedSyntax: .inlineOnlyPreservingWhitespace,
                failurePolicy: .returnPartiallyParsedIfPossible
            )
        )

        var attributed = parsed ?? AttributedString(text)
        styleRuns(&attributed, baseColor: baseColor)
        return attributed
    }

    private static func styleRuns(_ attributed: inout AttributedString, baseColor: Color) {
        for run in attributed.runs {
            let range = run.range
            let intent = run.inlinePresentationIntent

            if run.link != nil {
                attributed[range].foregroundColor = Color.blue.opacity(0.92)
                attributed[range].underlineStyle = .single
                continue
            }

            if intent?.contains(.code) == true {
                attributed[range].foregroundColor = Color.cyan.opacity(0.94)
                attributed[range].backgroundColor = Color.white.opacity(0.1)
                continue
            }

            if intent?.contains(.emphasized) == true {
                attributed[range].foregroundColor = Color.orange.opacity(0.92)
                continue
            }

            if intent?.contains(.stronglyEmphasized) == true {
                attributed[range].foregroundColor = Color.white
                continue
            }

            attributed[range].foregroundColor = baseColor
        }
    }

    private static func injectFileLinks(into text: String, sessionID: UUID?) -> String {
        guard let sessionID else {
            return text
        }

        let leadingTrimSet = CharacterSet(charactersIn: "([<{\"'")
        let trailingTrimSet = CharacterSet(charactersIn: ".,;!?)>]}\"'")

        var output = ""
        var index = text.startIndex
        while index < text.endIndex {
            let character = text[index]
            if character.isWhitespace {
                output.append(character)
                index = text.index(after: index)
                continue
            }

            let tokenStart = index
            while index < text.endIndex,
                  !text[index].isWhitespace {
                index = text.index(after: index)
            }

            let token = String(text[tokenStart..<index])
            let linked = linkedTokenMarkdown(
                token,
                sessionID: sessionID,
                leadingTrimSet: leadingTrimSet,
                trailingTrimSet: trailingTrimSet
            )
            output.append(linked ?? token)
        }

        return output
    }

    private static func linkedTokenMarkdown(
        _ token: String,
        sessionID: UUID,
        leadingTrimSet: CharacterSet,
        trailingTrimSet: CharacterSet
    ) -> String? {
        guard !token.contains("]("),
              !token.contains("::"),
              !token.hasPrefix("http://"),
              !token.hasPrefix("https://")
        else {
            return nil
        }

        var core = token
        var leading = ""
        var trailing = ""

        while let first = core.unicodeScalars.first,
              leadingTrimSet.contains(first) {
            leading.append(core.removeFirst())
        }

        while let last = core.unicodeScalars.last,
              trailingTrimSet.contains(last) {
            trailing.insert(core.removeLast(), at: trailing.startIndex)
        }

        if core.hasPrefix("`") && core.hasSuffix("`") && core.count > 2 {
            core.removeFirst()
            core.removeLast()
        }

        guard let fileReference = parseFileReference(from: core) else {
            return nil
        }

        let display = core
            .replacingOccurrences(of: "[", with: "\\[")
            .replacingOccurrences(of: "]", with: "\\]")
        let link = fileLinkURL(
            path: fileReference.path,
            line: fileReference.line,
            column: fileReference.column,
            sessionID: sessionID
        )

        guard !link.isEmpty else {
            return nil
        }

        return "\(leading)[\(display)](\(link))\(trailing)"
    }

    private static func parseFileReference(from candidate: String) -> FileReferenceToken? {
        guard !candidate.isEmpty,
              !candidate.contains("://")
        else {
            return nil
        }

        var path = candidate
        var line: Int?
        var column: Int?

        if let hashRange = path.range(of: "#L", options: .backwards) {
            let suffix = String(path[hashRange.upperBound...])
            if let components = parseLineColumnSuffix(suffix) {
                line = components.line
                column = components.column
                path = String(path[..<hashRange.lowerBound])
            }
        } else if let components = parseTrailingLineColumn(in: path) {
            path = components.path
            line = components.line
            column = components.column
        }

        let trimmedPath = path.trimmingCharacters(in: .whitespacesAndNewlines)
        guard isLikelyFilePath(trimmedPath) else {
            return nil
        }

        return FileReferenceToken(path: trimmedPath, line: line, column: column)
    }

    private static func parseTrailingLineColumn(in value: String) -> (path: String, line: Int?, column: Int?)? {
        let segments = value.split(separator: ":", omittingEmptySubsequences: false)
        guard segments.count >= 2,
              let last = segments.last,
              Int(last) != nil
        else {
            return nil
        }

        if segments.count >= 3,
           let column = Int(segments[segments.count - 1]),
           let line = Int(segments[segments.count - 2]) {
            let path = segments.dropLast(2).joined(separator: ":")
            return (path: path, line: line, column: column)
        }

        guard let parsedLine = Int(segments[segments.count - 1]) else {
            return nil
        }

        let path = segments.dropLast(1).joined(separator: ":")
        return (path: path, line: parsedLine, column: nil)
    }

    private static func parseLineColumnSuffix(_ value: String) -> (line: Int, column: Int?)? {
        if let columnMarker = value.range(of: "C") {
            let linePart = String(value[..<columnMarker.lowerBound])
            let columnPart = String(value[columnMarker.upperBound...])
            if let line = Int(linePart),
               let column = Int(columnPart) {
                return (line: line, column: column)
            }
            return nil
        }

        if let line = Int(value) {
            return (line: line, column: nil)
        }

        return nil
    }

    private static func isLikelyFilePath(_ value: String) -> Bool {
        guard !value.isEmpty,
              value != ".",
              value != ".."
        else {
            return false
        }

        if value.hasPrefix("~/") || value.hasPrefix("/") {
            return true
        }

        if value.contains("/") {
            return true
        }

        let lowercase = value.lowercased()
        let knownFileNames = [
            "cargo.toml", "cargo.lock", "package.json", "package-lock.json",
            "pnpm-lock.yaml", "yarn.lock", "readme.md"
        ]
        if knownFileNames.contains(lowercase) {
            return true
        }

        guard let dot = value.lastIndex(of: "."),
              dot > value.startIndex
        else {
            return false
        }

        let ext = value[value.index(after: dot)...]
        return (1...12).contains(ext.count)
    }

    private static func fileLinkURL(path: String, line: Int?, column: Int?, sessionID: UUID) -> String {
        var components = URLComponents()
        components.scheme = "code-native-file"
        components.host = "open"

        var queryItems = [
            URLQueryItem(name: "session", value: sessionID.uuidString),
            URLQueryItem(name: "path", value: path),
        ]

        if let line {
            queryItems.append(URLQueryItem(name: "line", value: String(line)))
        }
        if let column {
            queryItems.append(URLQueryItem(name: "column", value: String(column)))
        }

        components.queryItems = queryItems
        return components.url?.absoluteString ?? ""
    }
}

private struct AssistantCodeBlock: View {
    let language: String?
    let code: String
    @State private var isHovering = false

    private var displayLanguage: String {
        let trimmed = language?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return trimmed.isEmpty ? "code" : trimmed.lowercased()
    }

    private var showsCopyButton: Bool {
        #if os(macOS)
        return isHovering
        #else
        return true
        #endif
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Text(displayLanguage)
                    .font(.caption2.monospaced().weight(.semibold))
                    .foregroundStyle(Color.cyan.opacity(0.9))

                Spacer(minLength: 8)

                Button {
                    copyTextToPasteboard(code)
                } label: {
                    Image(systemName: "doc.on.doc")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.9))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 5)
                        .background(Color.white.opacity(0.08), in: Capsule(style: .continuous))
                }
                .buttonStyle(.plain)
                .opacity(showsCopyButton ? 1 : 0)
                .animation(.easeInOut(duration: 0.14), value: showsCopyButton)
                .allowsHitTesting(showsCopyButton)
                .accessibilityLabel("Copy code block")
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(Color.white.opacity(0.04))

            ScrollView(.horizontal, showsIndicators: false) {
                Text(code)
                    .font(.callout.monospaced())
                    .foregroundStyle(Color.green.opacity(0.92))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 10)
            }
        }
        .background(Color.black.opacity(0.26), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(Color.white.opacity(0.08), lineWidth: 1)
        )
        #if os(macOS)
        .onHover { hovering in
            isHovering = hovering
        }
        #endif
    }

    private func copyTextToPasteboard(_ value: String) {
        #if os(macOS)
        let board = NSPasteboard.general
        board.clearContents()
        board.setString(value, forType: .string)
        #elseif os(iOS)
        UIPasteboard.general.string = value
        #endif
    }
}

private struct BeautifulDiffPreview: View {
    let diffText: String
    let visibleLineLimit: Int?

    private struct PreviewRow {
        let sourceIndex: Int?
        let text: String
        let kind: DiffLineKind
        let isFoldMarker: Bool
    }

    private struct ParsedDiff {
        let contentLines: [String]
        let contentRows: [PreviewRow]
        let foldedRows: [PreviewRow]
        let filePaths: [String]
        let additions: Int
        let removals: Int
    }

    private let parsed: ParsedDiff

    init(diffText: String, visibleLineLimit: Int?) {
        self.diffText = diffText
        self.visibleLineLimit = visibleLineLimit
        self.parsed = Self.parseDiff(diffText)
    }

    private var previewRows: [PreviewRow] {
        guard let visibleLineLimit else {
            return parsed.contentRows
        }

        return normalizePreviewRows(focusedRows(from: parsed.foldedRows, maxLines: visibleLineLimit))
    }

    private var syntaxHighlightEnabled: Bool {
        if previewRows.count > 180 {
            return false
        }

        return !previewRows.contains(where: { $0.text.count > 320 })
    }

    private var hiddenLineCount: Int {
        let shown = Set(previewRows.compactMap(\.sourceIndex)).count
        return max(0, parsed.contentLines.count - shown)
    }

    var fileSummaryText: String {
        guard let first = parsed.filePaths.first else {
            return "Diff preview"
        }

        return compactPath(first)
    }

    var extraFileCount: Int {
        max(parsed.filePaths.count - 1, 0)
    }

    var additionsCount: Int {
        parsed.additions
    }

    var removalsCount: Int {
        parsed.removals
    }

    var hiddenLinesCount: Int {
        hiddenLineCount
    }

    var totalLinesCount: Int {
        parsed.contentLines.count
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            VStack(spacing: 3) {
                ForEach(Array(previewRows.enumerated()), id: \.offset) { _, row in
                    DiffLineRow(
                        text: row.text,
                        kind: row.kind,
                        isFoldMarker: row.isFoldMarker,
                        syntaxHighlightEnabled: syntaxHighlightEnabled
                    )
                }
            }
        }
    }

    private func compactPath(_ path: String) -> String {
        let components = path.split(separator: "/")
        if components.count <= 2 {
            return path
        }

        return components.suffix(2).joined(separator: "/")
    }

    static func isMetadataLine(_ line: String) -> Bool {
        if line.hasPrefix("diff --git ") || line.hasPrefix("index ") || line.hasPrefix("---") || line.hasPrefix("+++") || line.hasPrefix("@@") {
            return true
        }

        if line.hasPrefix("new file mode ") || line.hasPrefix("deleted file mode ") || line.hasPrefix("rename from ") || line.hasPrefix("rename to ") {
            return true
        }

        return false
    }

    private static func foldContextRows(lines: [String]) -> [PreviewRow] {
        var rows: [PreviewRow] = []
        var index = 0

        while index < lines.count {
            let line = lines[index]
            let kind = DiffLineKind(text: line)
            if kind != .context {
                rows.append(PreviewRow(sourceIndex: index, text: line, kind: kind, isFoldMarker: false))
                index += 1
                continue
            }

            let start = index
            while index < lines.count,
                  DiffLineKind(text: lines[index]) == .context {
                index += 1
            }

            let run = lines[start..<index]
            if run.count <= 8 {
                rows.append(contentsOf: run.enumerated().map { offset, line in
                    PreviewRow(
                        sourceIndex: start + offset,
                        text: line,
                        kind: .context,
                        isFoldMarker: false
                    )
                })
                continue
            }

            let head = run.prefix(2)
            let tail = run.suffix(2)
            rows.append(contentsOf: head.enumerated().map { offset, line in
                PreviewRow(
                    sourceIndex: start + offset,
                    text: line,
                    kind: .context,
                    isFoldMarker: false
                )
            })

            rows.append(
                PreviewRow(
                    sourceIndex: nil,
                    text: "…",
                    kind: .foldMarker,
                    isFoldMarker: true
                )
            )

            rows.append(contentsOf: tail.enumerated().map { offset, line in
                PreviewRow(
                    sourceIndex: index - tail.count + offset,
                    text: line,
                    kind: .context,
                    isFoldMarker: false
                )
            })
        }

        return rows
    }

    private func focusedRows(from rows: [PreviewRow], maxLines: Int) -> [PreviewRow] {
        guard rows.count > maxLines else {
            return rows
        }

        let importantKinds: Set<DiffLineKind> = [.added, .removed]
        let importantIndices = rows.indices.filter { importantKinds.contains(rows[$0].kind) }
        guard !importantIndices.isEmpty else {
            return Array(rows.prefix(maxLines))
        }

        if maxLines <= 14 {
            let changedRows = rows.filter { importantKinds.contains($0.kind) }
            let keepCount = max(1, maxLines - 1)

            if changedRows.count <= keepCount {
                return changedRows
            }

            let headCount = max(1, keepCount / 2)
            let tailCount = max(0, keepCount - headCount)
            let head = Array(changedRows.prefix(headCount))
            let tail = tailCount > 0 ? Array(changedRows.suffix(tailCount)) : []

            let visibleSource = Set((head + tail).compactMap(\.sourceIndex)).count
            let totalSource = Set(rows.compactMap(\.sourceIndex)).count
            let hiddenSource = max(0, totalSource - visibleSource)

            if hiddenSource == 0 {
                return head + tail
            }

            return head + [
                PreviewRow(
                    sourceIndex: nil,
                    text: "…",
                    kind: .foldMarker,
                    isFoldMarker: true
                )
            ] + tail
        }

        let includeContext = maxLines > 16

        var selected: Set<Int> = []
        for index in importantIndices {
            selected.insert(index)
            if includeContext {
                let before = index - 1
                if before >= rows.startIndex,
                   rows[before].kind == .context {
                    selected.insert(before)
                }

                let after = index + 1
                if after < rows.endIndex,
                   rows[after].kind == .context {
                    selected.insert(after)
                }
            }
        }

        let ordered = selected.sorted()
        var compacted: [PreviewRow] = []
        var cursor: Int?

        for index in ordered {
            if let cursor,
               index > cursor + 1 {
                let hidden = index - cursor - 1
                if includeContext && hidden <= 5 {
                    for fillIndex in (cursor + 1)..<index {
                        compacted.append(rows[fillIndex])
                    }
                } else if hidden >= 12 {
                    compacted.append(
                        PreviewRow(
                            sourceIndex: nil,
                            text: "…",
                            kind: .foldMarker,
                            isFoldMarker: true
                        )
                    )
                }
            }

            compacted.append(rows[index])
            cursor = index
        }

        if compacted.count <= maxLines {
            return compacted
        }

        let prefix = Array(compacted.prefix(maxLines - 1))
        return prefix + [
            PreviewRow(
                sourceIndex: nil,
                text: "…",
                kind: .foldMarker,
                isFoldMarker: true
            )
        ]
    }

    private static func parseDiff(_ diffText: String) -> ParsedDiff {
        let lines = diffText.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)

        var additions = 0
        var removals = 0
        var filePaths: [String] = []
        var seenPaths: Set<String> = []

        for line in lines {
            if line.hasPrefix("+") && !line.hasPrefix("+++") {
                additions += 1
            }
            if line.hasPrefix("-") && !line.hasPrefix("---") {
                removals += 1
            }

            if let path = extractPathFromDiffHeader(line),
               !seenPaths.contains(path) {
                seenPaths.insert(path)
                filePaths.append(path)
            }
        }

        let contentLines = {
            let trimmed = lines.filter { line in
                !isMetadataLine(line)
            }
            return trimmed.isEmpty ? lines : trimmed
        }()

        let contentRows = contentLines.enumerated().map { index, line in
            PreviewRow(
                sourceIndex: index,
                text: line,
                kind: DiffLineKind(text: line),
                isFoldMarker: false
            )
        }

        let foldedRows = foldContextRows(lines: contentLines)

        return ParsedDiff(
            contentLines: contentLines,
            contentRows: contentRows,
            foldedRows: foldedRows,
            filePaths: filePaths,
            additions: additions,
            removals: removals
        )
    }

    private func normalizePreviewRows(_ rows: [PreviewRow]) -> [PreviewRow] {
        guard !rows.isEmpty else {
            return rows
        }

        var trimmed = rows
        while trimmed.first?.isFoldMarker == true {
            trimmed.removeFirst()
        }
        while trimmed.last?.isFoldMarker == true {
            trimmed.removeLast()
        }

        if trimmed.isEmpty {
            return rows
        }

        var compacted: [PreviewRow] = []
        compacted.reserveCapacity(trimmed.count)
        for row in trimmed {
            if row.isFoldMarker,
               compacted.last?.isFoldMarker == true {
                continue
            }
            compacted.append(row)
        }

        return compacted
    }

    private static func extractPathFromDiffHeader(_ line: String) -> String? {
        guard line.hasPrefix("diff --git ") else {
            return nil
        }

        let parts = line.split(separator: " ", omittingEmptySubsequences: true)
        guard parts.count >= 4 else {
            return nil
        }

        var path = String(parts[3])
        if path.hasPrefix("b/") {
            path.removeFirst(2)
        }
        return path
    }

}

private struct DiffSummaryPill: View {
    let fileText: String
    let extraFileCount: Int
    let additions: Int
    let removals: Int

    var body: some View {
        HStack(spacing: 7) {
            Image(systemName: "folder")
                .foregroundStyle(Color.blue.opacity(0.82))

            Text(fileText)
                .foregroundStyle(Color.blue.opacity(0.96))
                .lineLimit(1)
                .truncationMode(.middle)

            if extraFileCount > 0 {
                Text("· \(extraFileCount + 1) files")
                    .foregroundStyle(.secondary)
            }

            Text("·")
                .foregroundStyle(.secondary)

            Text("+\(additions)")
                .foregroundStyle(Color.green.opacity(0.95))

            Text("-\(removals)")
                .foregroundStyle(Color.red.opacity(0.92))
        }
        .font(.caption2.monospaced())
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(Color.white.opacity(0.08), in: Capsule(style: .continuous))
    }
}

private struct DiffLineRow: View {
    let text: String
    let kind: DiffLineKind
    let isFoldMarker: Bool
    let syntaxHighlightEnabled: Bool

    private var isBlankContextLine: Bool {
        kind == .context && text.trimmingCharacters(in: .whitespaces).isEmpty
    }

    private struct SyntaxSegment {
        let text: String
        let color: Color
    }

    private static let keywords: Set<String> = [
        "fn", "let", "mut", "pub", "struct", "enum", "impl", "match", "if", "else", "for", "while", "loop", "return",
        "func", "var", "let", "struct", "enum", "class", "protocol", "extension", "if", "else", "guard", "for", "while", "switch", "case", "return",
        "const", "static", "void", "int", "float", "double", "bool", "true", "false", "null", "undefined"
    ]

    private var syntaxHighlightedText: Text {
        let base = kind.foreground
        let segments = tokenize(text: text, base: base)
        var attributed = AttributedString()
        for segment in segments {
            var chunk = AttributedString(segment.text)
            chunk.foregroundColor = segment.color
            attributed += chunk
        }
        return Text(attributed)
    }

    var body: some View {
        if isFoldMarker {
            HStack {
                Spacer(minLength: 0)
                Text(text)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(Color.white.opacity(0.42))
                Spacer(minLength: 0)
            }
            .padding(.vertical, 1)
        } else if isBlankContextLine {
            RoundedRectangle(cornerRadius: 2, style: .continuous)
                .fill(Color.white.opacity(0.045))
                .frame(height: 6)
                .padding(.horizontal, 12)
                .padding(.vertical, 2)
        } else {
            HStack(alignment: .top, spacing: 0) {
                syntaxHighlightedText
                    .font(.caption.monospaced())
                    .textSelection(.enabled)
                    .lineLimit(nil)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: isFoldMarker ? .center : .leading)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(kind.background, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
            .overlay(alignment: .leading) {
                if !isFoldMarker {
                    RoundedRectangle(cornerRadius: 3, style: .continuous)
                        .fill(kind.accent)
                        .frame(width: 3)
                        .padding(.vertical, 2)
                }
            }
        }
    }

    private func tokenize(text: String, base: Color) -> [SyntaxSegment] {
        if !syntaxHighlightEnabled {
            return [SyntaxSegment(text: text.isEmpty ? " " : text, color: base)]
        }

        guard !isFoldMarker else {
            return [SyntaxSegment(text: text.isEmpty ? " " : text, color: kind.foreground)]
        }

        if text.isEmpty {
            return [SyntaxSegment(text: " ", color: base)]
        }

        var segments: [SyntaxSegment] = []
        var current = ""
        var inString = false
        var inComment = false

        func flushCurrent() {
            guard !current.isEmpty else {
                return
            }

            let token = current
            current.removeAll(keepingCapacity: true)

            if inComment {
                segments.append(SyntaxSegment(text: token, color: .secondary.opacity(0.9)))
                return
            }

            if inString {
                segments.append(SyntaxSegment(text: token, color: Color.orange.opacity(0.9)))
                return
            }

            if Self.keywords.contains(token) {
                segments.append(SyntaxSegment(text: token, color: Color.cyan.opacity(0.95)))
                return
            }

            if Double(token.replacingOccurrences(of: "_", with: "")) != nil {
                segments.append(SyntaxSegment(text: token, color: Color.purple.opacity(0.85)))
                return
            }

            segments.append(SyntaxSegment(text: token, color: base))
        }

        let chars = Array(text)
        var index = 0
        while index < chars.count {
            let ch = chars[index]

            if inComment {
                current.append(ch)
                index += 1
                continue
            }

            if !inString,
               ch == "/",
               index + 1 < chars.count,
               chars[index + 1] == "/" {
                flushCurrent()
                inComment = true
                current.append(ch)
                current.append(chars[index + 1])
                index += 2
                continue
            }

            if ch == "\"" {
                flushCurrent()
                inString.toggle()
                current.append(ch)
                index += 1
                continue
            }

            if ch.isLetter || ch.isNumber || ch == "_" || (inString && ch != "\"") {
                current.append(ch)
                index += 1
                continue
            }

            flushCurrent()
            segments.append(SyntaxSegment(text: String(ch), color: base))
            index += 1
        }

        flushCurrent()
        return segments
    }
}

private enum DiffLineKind: Hashable {
    case fileHeader
    case hunk
    case added
    case removed
    case meta
    case context
    case foldMarker

    init(text: String) {
        if text.hasPrefix("diff --git ") {
            self = .fileHeader
            return
        }
        if text.hasPrefix("@@") {
            self = .hunk
            return
        }
        if text.hasPrefix("+") && !text.hasPrefix("+++") {
            self = .added
            return
        }
        if text.hasPrefix("-") && !text.hasPrefix("---") {
            self = .removed
            return
        }
        if text.hasPrefix("index ") || text.hasPrefix("---") || text.hasPrefix("+++") {
            self = .meta
            return
        }
        self = .context
    }

    var background: Color {
        switch self {
        case .fileHeader:
            return Color.white.opacity(0.07)
        case .hunk:
            return Color.white.opacity(0.06)
        case .added:
            return Color.green.opacity(0.075)
        case .removed:
            return Color.red.opacity(0.075)
        case .meta:
            return Color.white.opacity(0.05)
        case .context:
            return Color.white.opacity(0.02)
        case .foldMarker:
            return Color.white.opacity(0.028)
        }
    }

    var foreground: Color {
        switch self {
        case .fileHeader:
            return Color.white.opacity(0.92)
        case .hunk:
            return Color.blue.opacity(0.88)
        case .added:
            return Color.green.opacity(0.92)
        case .removed:
            return Color.red.opacity(0.90)
        case .meta:
            return Color.white.opacity(0.78)
        case .context:
            return Color.white.opacity(0.86)
        case .foldMarker:
            return Color.white.opacity(0.46)
        }
    }

    var accent: Color {
        switch self {
        case .fileHeader:
            return Color.white.opacity(0.35)
        case .hunk:
            return Color.blue.opacity(0.65)
        case .added:
            return Color.green.opacity(0.75)
        case .removed:
            return Color.red.opacity(0.75)
        case .meta:
            return Color.white.opacity(0.24)
        case .context:
            return Color.clear
        case .foldMarker:
            return Color.white.opacity(0.28)
        }
    }
}

private func normalizedStructuredActivityLines(from value: String) -> [String] {
    var normalized = value.replacingOccurrences(of: "\r\n", with: "\n")
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
        .components(separatedBy: "\n")
        .compactMap { line in
            let withoutTrailingWhitespace = line.replacingOccurrences(
                of: "\\s+$",
                with: "",
                options: .regularExpression
            )
            let hasVisibleContent = !withoutTrailingWhitespace
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .isEmpty
            if !hasVisibleContent {
                return nil
            }
            return withoutTrailingWhitespace
        }
}

private struct TranscriptCard: View {
    private enum RequestInputSubmissionState {
        case idle
        case submitted
    }

    let item: SessionStreamItem
    let isActive: Bool
    let density: TranscriptDensity
    let taskActivityLines: [String]
    let onActivate: () -> Void
    let onApproval: (ApprovalDecisionChoice) -> Void
    let onRequestUserInputResponse: ([String: [String]]) -> Void

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    @State private var selectedDecision: ApprovalDecisionChoice = .approved
    @State private var requestInputSelections: [String: String] = [:]
    @State private var requestInputNotes: [String: String] = [:]
    @State private var requestInputSubmissionState: RequestInputSubmissionState = .idle
    @State private var isExpanded = false
    @State private var diffExpansionSteps = 0
    @State private var copiedDiffRecoveryCommandLabel: String?
    @State private var isHoveringCopyActions = false

    private var effectiveLineSpacing: CGFloat {
        usesCompactPhoneLayout ? min(2, density.lineSpacing) : density.lineSpacing
    }

    private var effectiveCardHorizontalPadding: CGFloat {
        usesCompactPhoneLayout ? 12 : density.cardHorizontalPadding
    }

    private var effectiveCardVerticalPadding: CGFloat {
        usesCompactPhoneLayout ? 10 : density.cardVerticalPadding
    }

    private var effectiveCardCornerRadius: CGFloat {
        usesCompactPhoneLayout ? 14 : 16
    }

    private var usesCompactPhoneLayout: Bool {
        #if os(iOS)
        UIDevice.current.userInterfaceIdiom == .phone && horizontalSizeClass == .compact
        #else
        false
        #endif
    }

    private var cardBackground: Color {
        if item.isTaskLifecycleEvent {
            return Color.white.opacity(0.04)
        }

        if item.isTurnAbortedEvent {
            return Color.orange.opacity(0.13)
        }

        if item.isPatchApplyEndEvent {
            return Color.white.opacity(0.045)
        }

        if item.isTokenCountEvent {
            return Color.white.opacity(0.035)
        }

        if item.isBackgroundEvent {
            return Color.white.opacity(0.04)
        }

        switch item.cardStyle {
        case .user:
            return Color.white.opacity(0.11)
        case .assistant:
            return Color.clear
        case .reasoning:
            return Color.white.opacity(0.02)
        case .tool:
            return Color.cyan.opacity(0.045)
        case .approval:
            return Color.orange.opacity(0.18)
        case .composer:
            return Color.gray.opacity(0.07)
        case .system:
            return Color.gray.opacity(0.08)
        case .defaultStyle:
            return Color.white.opacity(0.05)
        }
    }

    private var cardBorder: Color {
        if item.isTaskLifecycleEvent {
            return Color.white.opacity(0.12)
        }

        if item.isTurnAbortedEvent {
            return Color.orange.opacity(0.34)
        }

        if item.isPatchApplyEndEvent {
            return Color.white.opacity(0.10)
        }

        if item.isTokenCountEvent {
            return Color.white.opacity(0.08)
        }

        if item.isBackgroundEvent {
            return Color.white.opacity(0.10)
        }

        switch item.cardStyle {
        case .user:
            return Color.white.opacity(0.18)
        case .assistant:
            return Color.white.opacity(0.18)
        case .reasoning:
            return Color.white.opacity(0.08)
        case .tool:
            return Color.cyan.opacity(0.14)
        case .approval:
            return Color.orange.opacity(0.52)
        case .composer:
            return Color.gray.opacity(0.18)
        case .system:
            return Color.gray.opacity(0.20)
        case .defaultStyle:
            return Color.white.opacity(0.14)
        }
    }

    private var usesMonospacedBody: Bool {
        if item.isTaskLifecycleEvent {
            return false
        }

        if item.isPatchApplyEndEvent {
            return false
        }

        if item.isTokenCountEvent {
            return false
        }

        if item.isBackgroundEvent {
            return false
        }

        switch item.cardStyle {
        case .tool, .system:
            return true
        case .reasoning, .user, .assistant, .approval, .composer, .defaultStyle:
            return false
        }
    }

    private var usesCompactBodyText: Bool {
        item.isPatchApplyEndEvent || item.isTokenCountEvent || item.isBackgroundEvent || item.isTaskLifecycleEvent
    }

    private var monospacedBodyFont: Font {
        #if os(iOS)
        if usesCompactPhoneLayout {
            return .system(.callout, design: .monospaced)
        }
        #endif
        return .body.monospaced()
    }

    private var showsMetaHeader: Bool {
        if item.isTaskLifecycleEvent {
            return false
        }

        if item.turnDiffText != nil {
            return false
        }

        if item.isReplayHistoryEvent {
            return false
        }

        if item.execCommandInfo != nil {
            return false
        }

        if item.isPatchApplyEndEvent {
            return false
        }

        if item.isTokenCountEvent {
            return false
        }

        if item.isBackgroundEvent {
            return false
        }

        switch item.cardStyle {
        case .assistant, .user, .defaultStyle:
            return false
        case .reasoning:
            return false
        case .tool, .approval, .composer, .system:
            return true
        }
    }

    private var metaHeaderColor: Color {
        if item.isTurnAbortedEvent {
            return Color.orange.opacity(0.9)
        }
        return .secondary
    }

    private var shouldDrawBorder: Bool {
        if item.isTaskLifecycleEvent {
            return true
        }

        if item.isBackgroundEvent {
            return true
        }

        switch item.cardStyle {
        case .assistant, .defaultStyle:
            return false
        case .reasoning:
            return false
        case .user, .tool, .approval, .composer, .system:
            return true
        }
    }

    private var cardFillGradient: LinearGradient {
        if cardBackground == .clear {
            return LinearGradient(colors: [.clear, .clear], startPoint: .topLeading, endPoint: .bottomTrailing)
        }

        return LinearGradient(
            colors: [
                cardBackground.opacity(0.96),
                cardBackground.opacity(0.78),
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
    }

    private var cardShadowColor: Color {
        if item.cardStyle == .assistant || cardBackground == .clear {
            return .clear
        }
        return Color.black.opacity(isActive ? 0.30 : 0.18)
    }

    private var cardShadowRadius: CGFloat {
        isActive ? 14 : 8
    }

    private var cardShadowY: CGFloat {
        isActive ? 8 : 4
    }

    private var collapsedBodyLimit: Int {
        if item.isTaskLifecycleEvent {
            return 420
        }

        if item.isPatchApplyEndEvent {
            return 500
        }

        if item.isTokenCountEvent {
            return 500
        }

        if item.isBackgroundEvent {
            return 340
        }

        if item.turnDiffText != nil {
            return isActive ? 800 : 240
        }

        switch item.cardStyle {
        case .tool, .reasoning, .system, .defaultStyle:
            return isActive ? 1_200 : 360
        case .approval, .composer, .assistant, .user:
            return 5_000
        }
    }

    private var isLongBody: Bool {
        if renderedAsDiff {
            return false
        }

        if item.body.count > collapsedBodyLimit {
            return true
        }

        if let bodyLineLimit,
           estimatedBodyLineCount > bodyLineLimit {
            return true
        }

        return false
    }

    private var displayedBody: String {
        guard !isExpanded else {
            return item.body
        }

        guard item.body.count > collapsedBodyLimit else {
            return item.body
        }

        let end = item.body.index(item.body.startIndex, offsetBy: collapsedBodyLimit)
        let hidden = item.body.count - collapsedBodyLimit
        return "\(item.body[..<end])\n\n… \(hidden) more characters"
    }

    private var estimatedBodyLineCount: Int {
        let wrapWidth = usesMonospacedBody ? 88 : 96
        return item.body
            .components(separatedBy: "\n")
            .reduce(into: 0) { count, line in
                let lineLength = line.count
                let wrapped = max(1, (lineLength + wrapWidth - 1) / wrapWidth)
                count += wrapped
            }
    }

    private var renderedAsDiff: Bool {
        item.turnDiffText != nil
    }

    private var copyableBodyText: String? {
        if let diff = item.turnDiffText {
            let trimmed = diff.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : diff
        }

        let trimmed = item.body.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return nil
        }

        return item.body
    }

    private var totalDiffLines: Int {
        guard let diff = item.turnDiffText else {
            return 0
        }

        let lines = diff.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        let filtered = lines.filter { !BeautifulDiffPreview.isMetadataLine($0) }
        return (filtered.isEmpty ? lines : filtered).count
    }

    private var baseDiffLineLimit: Int {
        if usesCompactPhoneLayout {
            return isActive ? 9 : 5
        }
        return isActive ? 12 : 7
    }

    private var diffExpandChunkSize: Int {
        usesCompactPhoneLayout ? 14 : 20
    }

    private var currentDiffLineLimit: Int {
        guard renderedAsDiff else {
            return 0
        }

        guard isActive else {
            return min(totalDiffLines, 6)
        }

        let expanded = baseDiffLineLimit + diffExpansionSteps * diffExpandChunkSize
        return min(totalDiffLines, expanded)
    }

    private var shouldClampBodyLines: Bool {
        switch item.cardStyle {
        case .tool, .reasoning, .system, .defaultStyle:
            return !renderedAsDiff && !isExpanded
        case .user:
            return usesCompactPhoneLayout && !isExpanded
        case .approval, .composer, .assistant:
            return false
        }
    }

    private var bodyLineLimit: Int? {
        guard shouldClampBodyLines else {
            return nil
        }
        if item.cardStyle == .user {
            return isActive ? 12 : 8
        }
        if usesCompactPhoneLayout {
            return isActive ? 14 : 6
        }
        return isActive ? 18 : 7
    }

    private var showsCopyActions: Bool {
        guard copyableBodyText != nil else {
            return false
        }

        #if os(macOS)
        return isActive || isHoveringCopyActions
        #else
        return isActive
        #endif
    }

    var body: some View {
        VStack(alignment: .leading, spacing: max(8, density.rowSpacing - 2)) {
            if showsMetaHeader {
                HStack {
                    Text(item.title)
                        .font(.caption.weight(.semibold))
                        .textCase(.uppercase)
                        .foregroundStyle(metaHeaderColor)

                    Spacer()
                }
            }

            if let diff = item.turnDiffText {
                let preview = BeautifulDiffPreview(
                    diffText: diff,
                    visibleLineLimit: isExpanded ? nil : currentDiffLineLimit
                )
                let hasHiddenDiffLines = preview.hiddenLinesCount > 0
                let showMoreCount = min(diffExpandChunkSize, preview.hiddenLinesCount)
                let canExpandDiff = isActive && hasHiddenDiffLines && !isExpanded
                let canCollapseDiff = isActive && (diffExpansionSteps > 0 || isExpanded)

                HStack {
                    DiffSummaryPill(
                        fileText: preview.fileSummaryText,
                        extraFileCount: preview.extraFileCount,
                        additions: preview.additionsCount,
                        removals: preview.removalsCount
                    )
                    Spacer()

                    if !isActive && hasHiddenDiffLines {
                        HStack(spacing: 6) {
                            Button("Show \(showMoreCount) more lines") {
                                diffExpansionSteps = 1
                                isExpanded = false
                                onActivate()
                            }
                            .buttonStyle(.plain)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.white.opacity(0.82))
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(Color.white.opacity(0.07), in: Capsule(style: .continuous))

                            Button("Show all \(preview.totalLinesCount) lines") {
                                isExpanded = true
                                onActivate()
                            }
                            .buttonStyle(.plain)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(Color.white.opacity(0.04), in: Capsule(style: .continuous))
                            .overlay(
                                Capsule(style: .continuous)
                                    .stroke(Color.white.opacity(0.06), lineWidth: 1)
                            )
                        }
                    } else if canExpandDiff || canCollapseDiff {
                        if canExpandDiff {
                            HStack(spacing: 6) {
                                Button("Show \(showMoreCount) more lines") {
                                    diffExpansionSteps += 1
                                }
                                .buttonStyle(.plain)
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.white.opacity(0.82))
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(Color.white.opacity(0.07), in: Capsule(style: .continuous))

                                Button("Show all \(preview.totalLinesCount) lines") {
                                    isExpanded = true
                                }
                                .buttonStyle(.plain)
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.secondary)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(Color.white.opacity(0.04), in: Capsule(style: .continuous))
                                .overlay(
                                    Capsule(style: .continuous)
                                        .stroke(Color.white.opacity(0.06), lineWidth: 1)
                                )
                            }
                        } else if canCollapseDiff {
                            Button("Collapse") {
                                diffExpansionSteps = 0
                                isExpanded = false
                            }
                            .buttonStyle(.plain)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.white.opacity(0.82))
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(Color.white.opacity(0.07), in: Capsule(style: .continuous))
                        }
                    }
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
                .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))

                if let recoveryPlan = item.diffRecoveryPlan {
                    diffRecoveryContent(recoveryPlan)
                }

                preview
            } else if let tokenUsage = item.tokenUsageBreakdown {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 7) {
                        Image(systemName: "chart.bar.fill")
                            .font(.caption)
                            .foregroundStyle(Color.white.opacity(0.78))

                        Text(tokenUsageHeadline(tokenUsage))
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.white.opacity(0.86))
                            .lineLimit(1)

                        Spacer(minLength: 4)
                    }

                    HStack(spacing: 6) {
                        tokenMetricPill(label: "In", value: tokenUsage.input)
                        tokenMetricPill(label: "Out", value: tokenUsage.output)
                        tokenMetricPill(label: "Reason", value: tokenUsage.reasoning)

                        Spacer(minLength: 0)
                    }
                }
            } else if item.isTaskLifecycleEvent || item.isBackgroundEvent {
                taskActivityContent
            } else if let exec = item.execCommandInfo {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        Text("Exec")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)

                        Spacer()

                        if let duration = exec.duration,
                           !duration.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            Text(duration)
                                .font(.caption2.monospaced())
                                .foregroundStyle(.white.opacity(0.72))
                                .padding(.horizontal, 6)
                                .padding(.vertical, 3)
                                .background(Color.white.opacity(0.06), in: Capsule(style: .continuous))
                        }

                        Text(execStatusText(for: exec))
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(execStatusColor(for: exec))
                    }

                    if !exec.command.isEmpty {
                        Text(exec.command)
                            .font(.caption.monospaced())
                            .foregroundStyle(.white.opacity(0.9))
                            .padding(.horizontal, 10)
                            .padding(.vertical, 7)
                            .background(Color.white.opacity(0.045), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    }

                    let outputPreview = previewExecOutput(exec.output)

                    VStack(alignment: .leading, spacing: 6) {
                        Text(outputPreview.isEmpty ? "Output · none" : "Output preview")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.secondary)

                        if outputPreview.isEmpty {
                            Text("No stdout/stderr captured")
                                .font(.caption.monospaced())
                                .foregroundStyle(.white.opacity(0.62))
                                .padding(.horizontal, 10)
                                .padding(.vertical, 7)
                                .background(Color.black.opacity(0.14), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                        } else {
                            Text(outputPreview)
                                .font(.caption.monospaced())
                                .foregroundStyle(.white.opacity(0.86))
                                .lineSpacing(2)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 7)
                                .background(Color.black.opacity(0.18), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                    }
                }
            } else if item.isReplayHistoryEvent {
                let replayMessages = item.replayHistoryMessages
                VStack(alignment: .leading, spacing: 10) {
                    if replayMessages.isEmpty {
                        Text(item.body)
                            .font(.subheadline)
                            .foregroundStyle(.white.opacity(0.92))
                            .lineSpacing(3)
                            .textSelection(.enabled)
                    } else {
                        ForEach(replayMessages) { message in
                            VStack(alignment: .leading, spacing: 5) {
                                Text(replayRoleLabel(for: message.role))
                                    .font(.caption2.weight(.semibold))
                                    .foregroundStyle(.secondary)

                                if message.role == .assistant {
                                    Text(message.text)
                                        .font(.body)
                                        .foregroundStyle(.white.opacity(0.93))
                                        .lineSpacing(3)
                                        .textSelection(.enabled)
                                        .fixedSize(horizontal: false, vertical: true)
                                } else {
                                    Text(message.text)
                                        .font(.body)
                                        .foregroundStyle(.white.opacity(0.93))
                                        .lineSpacing(3)
                                        .textSelection(.enabled)
                                        .fixedSize(horizontal: false, vertical: true)
                                }
                            }
                            .padding(.horizontal, 10)
                            .padding(.vertical, 8)
                            .background(Color.white.opacity(0.03), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                    }
                }
            } else if let requestInputState = item.requestUserInputPromptState {
                requestUserInputContent(requestInputState)
            } else {
                Text(displayedBody)
                    .font(
                        usesMonospacedBody
                        ? monospacedBodyFont
                        : (usesCompactBodyText
                           ? (usesCompactPhoneLayout ? .caption : .subheadline)
                           : (usesCompactPhoneLayout ? .callout : .body))
                    )
                    .foregroundStyle(.white.opacity(0.93))
                    .lineSpacing(effectiveLineSpacing)
                    .textSelection(.enabled)
                    .lineLimit(bodyLineLimit)
                    .fixedSize(horizontal: false, vertical: true)

                if isLongBody {
                    Button(isExpanded ? "Collapse" : expandButtonLabel) {
                        isExpanded.toggle()
                    }
                    .buttonStyle(.plain)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.82))
                }
            }

            if item.approvalRequest != nil {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Approval required")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(Color.orange.opacity(0.95))

                    ForEach(Array(ApprovalDecisionChoice.allCases.enumerated()), id: \.offset) { index, choice in
                        Button {
                            selectedDecision = choice
                        } label: {
                            HStack(spacing: 8) {
                                Text("\(index + 1).")
                                    .font(.caption.monospaced())
                                    .foregroundStyle(.secondary)
                                    .frame(width: 20, alignment: .trailing)
                                Text(choice.label)
                                    .font(.subheadline)
                                    .foregroundStyle(.white.opacity(0.92))
                                Spacer()
                                if selectedDecision == choice {
                                    Image(systemName: "checkmark")
                                        .font(.caption.weight(.semibold))
                                        .foregroundStyle(.white.opacity(0.85))
                                }
                            }
                            .padding(.horizontal, 10)
                            .padding(.vertical, 8)
                            .background(
                                selectedDecision == choice
                                ? Color.orange.opacity(0.18)
                                : Color.white.opacity(0.05),
                                in: RoundedRectangle(cornerRadius: 10, style: .continuous)
                            )
                        }
                        .buttonStyle(.plain)
                        #if os(macOS)
                        .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: [])
                        #endif
                    }

                    HStack(spacing: 10) {
                        Button("Set deny") {
                            selectedDecision = .denied
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(.secondary)
                        #if os(macOS)
                        .keyboardShortcut("d", modifiers: [.command])
                        #endif

                        Spacer()

                        Button("Send decision") {
                            onApproval(selectedDecision)
                        }
                        .buttonStyle(.borderedProminent)
                        #if os(macOS)
                        .keyboardShortcut(.defaultAction)
                        #endif
                    }
                    .padding(.top, 4)
                }
                .padding(10)
                .background(Color.orange.opacity(0.10), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .stroke(Color.orange.opacity(0.40), lineWidth: 1)
                )
            }
        }
        .padding(.horizontal, effectiveCardHorizontalPadding)
        .padding(.vertical, effectiveCardVerticalPadding)
        .background(
            RoundedRectangle(cornerRadius: effectiveCardCornerRadius, style: .continuous)
                .fill(cardFillGradient)
        )
        .clipShape(RoundedRectangle(cornerRadius: effectiveCardCornerRadius, style: .continuous))
        .overlay {
            if shouldDrawBorder {
                RoundedRectangle(cornerRadius: effectiveCardCornerRadius, style: .continuous)
                    .stroke(cardBorder, lineWidth: 1)
            }
        }
        .overlay {
            if isActive {
                RoundedRectangle(cornerRadius: effectiveCardCornerRadius, style: .continuous)
                    .stroke(Color.white.opacity(0.16), lineWidth: 1)
            }
        }
        .overlay(alignment: .bottomTrailing) {
            if let copyableBodyText {
                Button {
                    copyTextToPasteboard(copyableBodyText)
                } label: {
                    Image(systemName: "doc.on.doc")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.86))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 6)
                        .background(Color.black.opacity(0.40), in: Capsule(style: .continuous))
                        .overlay(
                            Capsule(style: .continuous)
                                .stroke(Color.white.opacity(0.10), lineWidth: 1)
                        )
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Copy card text")
                .padding(.trailing, max(6, effectiveCardHorizontalPadding - 4))
                .padding(.bottom, max(4, effectiveCardVerticalPadding - 2))
                .opacity(showsCopyActions ? 1 : 0)
                .animation(.easeInOut(duration: 0.16), value: showsCopyActions)
                .allowsHitTesting(showsCopyActions)
            }
        }
        .shadow(color: cardShadowColor, radius: cardShadowRadius, x: 0, y: cardShadowY)
        .contextMenu {
            if !item.body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Button("Copy text") {
                    copyTextToPasteboard(item.body)
                }
            }
        }
        #if os(macOS)
        .onHover { hovering in
            isHoveringCopyActions = hovering
        }
        #endif
        .onTapGesture {
            onActivate()
        }
        .onChange(of: item.id) { _, _ in
            requestInputSelections = [:]
            requestInputNotes = [:]
            requestInputSubmissionState = .idle
        }
        .onChange(of: isActive) { _, active in
            if !active {
                diffExpansionSteps = 0
                isExpanded = false
                copiedDiffRecoveryCommandLabel = nil
            }
        }
    }

    private func copyTextToPasteboard(_ text: String) {
        #if os(macOS)
        let board = NSPasteboard.general
        board.clearContents()
        board.setString(text, forType: .string)
        #elseif os(iOS)
        UIPasteboard.general.string = text
        #endif
    }

    @ViewBuilder
    private func diffRecoveryContent(_ plan: DiffRecoveryPlan) -> some View {
        let previewPaths = Array(plan.changedPaths.prefix(3))
        let remainingPathCount = max(0, plan.changedPaths.count - previewPaths.count)
        let pathSummary = previewPaths.joined(separator: ", ")

        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: "arrow.uturn.backward.circle")
                    .font(.caption)
                    .foregroundStyle(Color.orange.opacity(0.92))

                Text("Diff recovery")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.9))

                Spacer(minLength: 8)

                Text("\(plan.changedPaths.count) file\(plan.changedPaths.count == 1 ? "" : "s")")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            Text(
                remainingPathCount > 0
                ? "\(pathSummary), +\(remainingPathCount) more"
                : pathSummary
            )
            .font(.caption.monospaced())
            .foregroundStyle(.white.opacity(0.82))
            .lineLimit(2)
            .truncationMode(.middle)

            HStack(spacing: 6) {
                diffRecoveryCommandButton(
                    title: "Copy review",
                    icon: "doc.text.magnifyingglass",
                    label: "review",
                    command: plan.reviewCommand,
                    tint: Color.white.opacity(0.08),
                    textTint: Color.white.opacity(0.88),
                    accessibilityID: "diff.recovery.copy-review"
                )

                diffRecoveryCommandButton(
                    title: "Copy snapshot",
                    icon: "square.and.arrow.down.on.square",
                    label: "snapshot",
                    command: plan.snapshotCommand,
                    tint: Color.cyan.opacity(0.16),
                    textTint: Color.cyan.opacity(0.98),
                    accessibilityID: "diff.recovery.copy-snapshot"
                )
            }

            HStack(spacing: 6) {
                diffRecoveryCommandButton(
                    title: "Copy restore",
                    icon: "arrow.uturn.backward",
                    label: "restore",
                    command: plan.restoreCommand,
                    tint: Color.orange.opacity(0.16),
                    textTint: Color.orange.opacity(0.96),
                    accessibilityID: "diff.recovery.copy-restore"
                )

                diffRecoveryCommandButton(
                    title: "Copy apply",
                    icon: "square.and.arrow.up",
                    label: "apply",
                    command: plan.applySnapshotCommand,
                    tint: Color.green.opacity(0.16),
                    textTint: Color.green.opacity(0.96),
                    accessibilityID: "diff.recovery.copy-apply"
                )
            }

            if let copiedDiffRecoveryCommandLabel {
                Text("Copied \(copiedDiffRecoveryCommandLabel) command")
                    .font(.caption2)
                    .foregroundStyle(Color.green.opacity(0.86))
                    .accessibilityIdentifier("diff.recovery.copy-status")
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 9)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .stroke(Color.white.opacity(0.10), lineWidth: 1)
        )
    }

    @ViewBuilder
    private func diffRecoveryCommandButton(
        title: String,
        icon: String,
        label: String,
        command: String,
        tint: Color,
        textTint: Color,
        accessibilityID: String
    ) -> some View {
        Button {
            copyTextToPasteboard(command)
            copiedDiffRecoveryCommandLabel = label
        } label: {
            Label(title, systemImage: icon)
                .font(.caption2.weight(.semibold))
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
                .background(tint, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                .foregroundStyle(textTint)
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(accessibilityID)
        .accessibilityHint(command)
    }

    @ViewBuilder
    private func requestUserInputContent(_ state: RequestUserInputPromptState) -> some View {
        switch state {
        case .ready(let request):
            requestUserInputReadyContent(request)
        case .loading(let callId, _):
            requestUserInputPlaceholderContent(
                icon: "hourglass",
                title: "Preparing questions",
                message: "Waiting for question payload before you can respond.",
                callId: callId,
                isError: false
            )
        case .empty(let callId, _):
            requestUserInputPlaceholderContent(
                icon: "questionmark.bubble",
                title: "No questions provided",
                message: "This prompt did not include any questions. Ask to retry with a complete request payload.",
                callId: callId,
                isError: true
            )
        case .invalid(let callId, _, let reason):
            requestUserInputPlaceholderContent(
                icon: "exclamationmark.triangle.fill",
                title: "Question payload is invalid",
                message: reason,
                callId: callId,
                isError: true
            )
        }
    }

    @ViewBuilder
    private func requestUserInputPlaceholderContent(
        icon: String,
        title: String,
        message: String,
        callId: String,
        isError: Bool
    ) -> some View {
        VStack(alignment: .leading, spacing: 9) {
            HStack(spacing: 8) {
                Image(systemName: icon)
                    .font(.caption)
                    .foregroundStyle(isError ? Color.orange.opacity(0.95) : Color.white.opacity(0.84))

                Text(title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(isError ? Color.orange.opacity(0.95) : .secondary)

                Spacer(minLength: 8)

                Text("request_user_input")
                    .font(.caption2.monospaced())
                    .foregroundStyle(.secondary)
            }

            Text(message)
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            Text("Call id: \(callId)")
                .font(.caption2.monospaced())
                .foregroundStyle(.white.opacity(0.78))
                .textSelection(.enabled)
        }
        .padding(10)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .stroke(isError ? Color.orange.opacity(0.34) : Color.white.opacity(0.1), lineWidth: 1)
        )
        .accessibilityIdentifier(isError ? "input.state.error" : "input.state.loading")
    }

    @ViewBuilder
    private func requestUserInputReadyContent(_ request: RequestUserInputPrompt) -> some View {
        let canSendResponse = canSubmitRequestUserInput(request)
        let isSubmitted = requestInputSubmissionState == .submitted

        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: "person.fill.questionmark")
                    .font(.caption)
                    .foregroundStyle(Color.orange.opacity(0.95))
                    .accessibilityLabel("User input required")
                Text("User input required")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                Spacer(minLength: 8)
                Text("\(request.questions.count) question\(request.questions.count == 1 ? "" : "s")")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            ForEach(Array(request.questions.enumerated()), id: \.element.id) { questionIndex, question in
                VStack(alignment: .leading, spacing: 7) {
                    Text(question.header)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.primary)

                    Text(question.question)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)

                    if !question.options.isEmpty {
                        ForEach(Array(question.options.enumerated()), id: \.element.label) { optionIndex, option in
                            let isSelected = requestInputSelections[question.id] == option.label
                            let shortcutDigit = requestInputShortcutDigit(
                                questionIndex: questionIndex,
                                optionIndex: optionIndex
                            )

                            let optionButton = Button {
                                withAnimation(.easeInOut(duration: 0.15)) {
                                    requestInputSelections[question.id] = option.label
                                }
                                requestInputSubmissionState = .idle
                            } label: {
                                HStack(spacing: 8) {
                                    Image(systemName: isSelected ? "circle.inset.filled" : "circle")
                                        .font(.caption)
                                        .foregroundStyle(isSelected ? Color.orange.opacity(0.9) : .secondary)
                                    VStack(alignment: .leading, spacing: 2) {
                                        HStack(spacing: 6) {
                                            Text(option.label)
                                                .font(.caption.weight(.semibold))
                                                .foregroundStyle(.primary)

                                            if let shortcutDigit {
                                                Text(shortcutDigit)
                                                    .font(.caption2.monospaced())
                                                    .foregroundStyle(.secondary)
                                                    .padding(.horizontal, 5)
                                                    .padding(.vertical, 2)
                                                    .background(Color.white.opacity(0.08), in: Capsule(style: .continuous))
                                            }
                                        }

                                        if !option.description.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                                            Text(option.description)
                                                .font(.caption2)
                                                .foregroundStyle(.secondary)
                                        }
                                    }
                                    Spacer(minLength: 0)
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 8)
                                .background(
                                    isSelected
                                    ? Color.orange.opacity(0.16)
                                    : Color.white.opacity(0.04),
                                    in: RoundedRectangle(cornerRadius: 8, style: .continuous)
                                )
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("input.option.\(question.id).\(option.label)")
                            .accessibilityLabel("\(option.label). \(option.description)")
                            .accessibilityHint(shortcutDigit.map { "Press \($0) to select this option" } ?? "")
                            .accessibilityAddTraits(isSelected ? .isSelected : [])

                            #if os(macOS)
                            if let shortcutDigit,
                               let shortcutKey = shortcutDigit.first {
                                optionButton
                                    .keyboardShortcut(KeyEquivalent(shortcutKey), modifiers: [])
                            } else {
                                optionButton
                            }
                            #else
                            optionButton
                            #endif
                        }
                    }

                    let noteBinding = Binding<String>(
                        get: { requestInputNotes[question.id] ?? "" },
                        set: {
                            requestInputNotes[question.id] = $0
                            requestInputSubmissionState = .idle
                        }
                    )

                    let notePlaceholder = question.options.isEmpty ? "Type your answer" : "Add a note (optional)"
                    if question.isSecret {
                        SecureField(notePlaceholder, text: noteBinding)
                            .textFieldStyle(.roundedBorder)
                            .accessibilityIdentifier("input.note.\(question.id)")
                            .accessibilityLabel("\(question.header) secret answer")
                    } else {
                        TextField(notePlaceholder, text: noteBinding)
                            .textFieldStyle(.roundedBorder)
                            .accessibilityIdentifier("input.note.\(question.id)")
                            .accessibilityLabel("\(question.header) response")
                    }
                }
                .padding(10)
                .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .accessibilityIdentifier("input.question.\(question.id)")
            }

            HStack(spacing: 10) {
                Button("Skip") {
                    submitRequestUserInput(request, skip: true)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(isSubmitted)
                .accessibilityIdentifier("input.skip")
                .accessibilityHint("Dismiss without answering")
                #if os(macOS)
                .keyboardShortcut(.escape, modifiers: [])
                #endif

                Spacer(minLength: 8)

                Button("Send response") {
                    submitRequestUserInput(request, skip: false)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .tint(.orange)
                .disabled(!canSendResponse || isSubmitted)
                .accessibilityIdentifier("input.send")
                .accessibilityHint(canSendResponse ? "Submit your answers" : "Select an option or type a response first")
                #if os(macOS)
                .keyboardShortcut(.defaultAction)
                #endif
            }

            if !canSendResponse && !isSubmitted {
                Text("Select an option or enter text to enable Send response.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("input.validation")
            }

            if isSubmitted {
                HStack(spacing: 6) {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.caption2)
                    Text("Response submitted")
                        .font(.caption2)
                }
                .foregroundStyle(Color.green.opacity(0.85))
                .accessibilityAddTraits(.updatesFrequently)
            }
        }
    }

    private func canSubmitRequestUserInput(_ request: RequestUserInputPrompt) -> Bool {
        for question in request.questions {
            if let selected = requestInputSelections[question.id]?.trimmingCharacters(in: .whitespacesAndNewlines),
               !selected.isEmpty {
                return true
            }

            if let note = requestInputNotes[question.id]?.trimmingCharacters(in: .whitespacesAndNewlines),
               !note.isEmpty {
                return true
            }
        }

        return false
    }

    private func submitRequestUserInput(_ request: RequestUserInputPrompt, skip: Bool) {
        requestInputSubmissionState = .submitted
        if skip {
            onRequestUserInputResponse([:])
            return
        }

        var answersByQuestionID: [String: [String]] = [:]
        for question in request.questions {
            var answers: [String] = []

            if let selected = requestInputSelections[question.id]?.trimmingCharacters(in: .whitespacesAndNewlines),
               !selected.isEmpty {
                answers.append(selected)
            }

            if let note = requestInputNotes[question.id]?.trimmingCharacters(in: .whitespacesAndNewlines),
               !note.isEmpty {
                answers.append(note)
            }

            if !answers.isEmpty {
                answersByQuestionID[question.id] = answers
            }
        }

        onRequestUserInputResponse(answersByQuestionID)
    }

    private func tokenUsageHeadline(_ usage: TokenUsageBreakdown) -> String {
        let model = item.tokenCountRequestedModel.map(displayTokenModelName) ?? "Token usage"
        guard let total = usage.total,
              total > 0
        else {
            return model
        }

        return "\(model) · \(formatCompactTokenCount(total)) total"
    }

    private func displayTokenModelName(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return "Token usage"
        }

        var formatted = trimmed
        if formatted.lowercased().hasPrefix("gpt-") {
            formatted = "GPT-\(formatted.dropFirst(4))"
        }
        formatted = formatted.replacingOccurrences(of: "-codex", with: "-Codex")
        formatted = formatted.replacingOccurrences(of: "-mini", with: "-Mini")
        return formatted
    }

    @ViewBuilder
    private func tokenMetricPill(label: String, value: Int?) -> some View {
        if let value,
           value > 0 {
            Text("\(label) \(formatCompactTokenCount(value))")
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.white.opacity(0.82))
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.white.opacity(0.06), in: Capsule(style: .continuous))
        }
    }

    private var expandButtonLabel: String {
        if bodyLineLimit != nil {
            return "Show more lines"
        }
        return "Show more"
    }

    private func previewExecOutput(_ value: String) -> String {
        let normalized = normalizeExecOutput(value)
        let trimmed = normalized.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return ""
        }

        let lines = trimmed.components(separatedBy: "\n")
        let head = lines.prefix(6)
        var preview = head.joined(separator: "\n")
        if lines.count > head.count {
            preview += "\n…"
        }
        return preview
    }

    private func normalizeExecOutput(_ value: String) -> String {
        var normalized = value.replacingOccurrences(of: "\r\n", with: "\n")
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

    private func normalizedActivityLines(from value: String) -> [String] {
        normalizedStructuredActivityLines(from: value)
    }

    private var taskActivityHeadline: String {
        let lines = normalizedActivityLines(from: item.body)
        return lines.first ?? (item.isTaskLifecycleEvent ? "Task activity" : "Background activity")
    }

    private var taskActivityDetails: String {
        let lines = normalizedActivityLines(from: item.body)
        let detailLines: [String]
        if item.isTaskLifecycleEvent {
            detailLines = Array(lines.dropFirst()) + taskActivityLines
        } else {
            detailLines = Array(lines.dropFirst())
        }

        return detailLines.joined(separator: "\n")
    }

    private var taskActivityDetailLines: [String] {
        taskActivityDetails
            .components(separatedBy: "\n")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    @ViewBuilder
    private var taskActivityContent: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: item.isTaskLifecycleEvent ? "hourglass" : "waveform.path.ecg")
                    .font(.caption)
                    .foregroundStyle(item.isTaskLifecycleEvent ? Color.orange.opacity(0.9) : Color.green.opacity(0.9))
                Text(taskActivityHeadline)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.95))
            }

            if !taskActivityDetails.isEmpty {
                VStack(alignment: .leading, spacing: 5) {
                    ForEach(Array(taskActivityDetailLines.enumerated()), id: \.offset) { _, line in
                        HStack(alignment: .firstTextBaseline, spacing: 7) {
                            Text("•")
                                .font(.caption2)
                                .foregroundStyle(Color.white.opacity(0.54))

                            Text(line)
                                .font(.caption.monospaced())
                                .foregroundStyle(.white.opacity(0.82))
                                .lineSpacing(2)
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
            }
        }
    }

    private func execStatusText(for info: ExecCommandInfo) -> String {
        guard let code = info.exitCode else {
            return "Completed"
        }

        if code == 0 {
            return "Success"
        }
        return "Exit \(code)"
    }

    private func execStatusColor(for info: ExecCommandInfo) -> Color {
        guard let code = info.exitCode else {
            return .secondary
        }

        return code == 0 ? Color.green.opacity(0.92) : Color.red.opacity(0.9)
    }

    private func replayRoleLabel(for role: ReplayHistoryMessage.Role) -> String {
        switch role {
        case .assistant:
            return "Assistant"
        case .user:
            return "You"
        case .unknown:
            return "Message"
        }
    }
}

private struct SlashCommandLauncherView: View {
    @Binding var query: String
    let commands: [ComposerSlashCommand]
    let onSelect: (ComposerSlashCommand) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var selectedCommandID: ComposerSlashCommand.ID?
    @FocusState private var searchFieldFocused: Bool

    private var selectedCommand: ComposerSlashCommand? {
        if let selectedCommandID,
           let matched = commands.first(where: { $0.id == selectedCommandID }) {
            return matched
        }
        return commands.first
    }

    private var selectedIndex: Int? {
        guard let selectedCommand else {
            return nil
        }
        return commands.firstIndex(where: { $0.id == selectedCommand.id })
    }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                TextField("Search slash commands", text: $query)
                    .textFieldStyle(.roundedBorder)
                    .focused($searchFieldFocused)
                    .accessibilityIdentifier("slash.search")
                    .accessibilityLabel("Filter slash commands")
                    .onSubmit {
                        applySelection()
                    }

                if commands.isEmpty {
                    Text("No matching commands")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                        .accessibilityIdentifier("slash.empty")
                } else {
                    List(Array(commands.enumerated()), id: \.element.id, selection: $selectedCommandID) { index, command in
                        Button {
                            selectedCommandID = command.id
                            onSelect(command)
                            dismiss()
                        } label: {
                            HStack(spacing: 0) {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(command.command)
                                        .font(.system(.body, design: .monospaced).weight(.semibold))
                                        .foregroundStyle(.primary)
                                    Text(command.summary)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(2)
                                }
                                Spacer(minLength: 8)
                                if index < 9 {
                                    Text("\(index + 1)")
                                        .font(.caption.monospaced().weight(.medium))
                                        .foregroundStyle(.tertiary)
                                        .padding(.horizontal, 6)
                                        .padding(.vertical, 3)
                                        .background(Color.secondary.opacity(0.12), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.vertical, 6)
                            .padding(.horizontal, 8)
                            .background(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .fill(selectedCommandID == command.id ? Color.accentColor.opacity(0.14) : Color.clear)
                            )
                        }
                        .buttonStyle(.plain)
                        .tag(command.id)
                        .accessibilityIdentifier("slash.item.\(command.command)")
                        .accessibilityLabel("\(command.title): \(command.summary)")
                        .accessibilityHint(index < 9 ? "Press \(index + 1) to select, or Return to select highlighted command" : "Press Return to select highlighted command")
                    }
                    .listStyle(.plain)

                    HStack(spacing: 8) {
                        Label("↑/↓", systemImage: "arrow.up.arrow.down")
                        Text("navigate")
                        Text("•")
                        Text("Return")
                        Text("select")
                        Text("•")
                        Text("Esc")
                        Text("close")
                        Text("•")
                        Text("1-9")
                        Text("quick pick")
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .accessibilityIdentifier("slash.keyboard-hints")
                }
            }
            .padding(14)
            .navigationTitle("Slash commands")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
            .overlay(alignment: .topLeading) {
                keyboardShortcutProxy
            }
            .onAppear {
                syncSelection()
                searchFieldFocused = true
            }
            .onChange(of: commands.map(\.id)) { _, _ in
                syncSelection()
            }
        }
    }

    @ViewBuilder
    private var keyboardShortcutProxy: some View {
        VStack(spacing: 0) {
            Button("Select slash command") {
                applySelection()
            }
            .keyboardShortcut(.defaultAction)
            .opacity(0.001)
            .frame(width: 1, height: 1)
            .disabled(commands.isEmpty)

            Button("Close slash commands") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)
            .opacity(0.001)
            .frame(width: 1, height: 1)

            Button("Previous slash command") {
                moveSelection(delta: -1)
            }
            .keyboardShortcut(.upArrow, modifiers: [])
            .opacity(0.001)
            .frame(width: 1, height: 1)
            .disabled(commands.count < 2)

            Button("Next slash command") {
                moveSelection(delta: 1)
            }
            .keyboardShortcut(.downArrow, modifiers: [])
            .opacity(0.001)
            .frame(width: 1, height: 1)
            .disabled(commands.count < 2)

            ForEach(Array(commands.prefix(9).enumerated()), id: \.element.id) { index, command in
                Button("Select \(command.command)") {
                    selectedCommandID = command.id
                    applySelection()
                }
                .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: [])
                .opacity(0.001)
                .frame(width: 1, height: 1)
            }
        }
        .accessibilityHidden(true)
    }

    private func syncSelection() {
        guard let first = commands.first else {
            selectedCommandID = nil
            return
        }

        guard let selectedCommandID,
              commands.contains(where: { $0.id == selectedCommandID })
        else {
            self.selectedCommandID = first.id
            return
        }
    }

    private func moveSelection(delta: Int) {
        guard !commands.isEmpty else {
            return
        }

        let fallbackIndex = delta >= 0 ? 0 : max(0, commands.count - 1)
        let currentIndex = selectedIndex ?? fallbackIndex
        let nextIndex = max(0, min(commands.count - 1, currentIndex + delta))
        selectedCommandID = commands[nextIndex].id
    }

    private func applySelection() {
        guard let selectedCommand else {
            return
        }

        onSelect(selectedCommand)
        dismiss()
    }
}

private struct ContextReferencePickerView: View {
    @Binding var query: String
    let candidates: [String]
    let isLoading: Bool
    let onRefresh: () -> Void
    let onSelect: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var selectedPath: String?
    @FocusState private var searchFocused: Bool

    private var selectedCandidate: String? {
        if let selectedPath,
           candidates.contains(selectedPath) {
            return selectedPath
        }
        return candidates.first
    }

    private var selectedIndex: Int? {
        guard let selectedCandidate else {
            return nil
        }
        return candidates.firstIndex(of: selectedCandidate)
    }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                TextField("Search files for @context", text: $query)
                    .textFieldStyle(.roundedBorder)
                    .focused($searchFocused)
                    .accessibilityIdentifier("context.search")
                    .accessibilityLabel("Filter workspace files")
                    .onSubmit {
                        applySelectedCandidate()
                    }

                if isLoading {
                    HStack(spacing: 8) {
                        ProgressView()
                            .controlSize(.small)
                        Text("Indexing workspace files…")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                if candidates.isEmpty {
                    Text(isLoading ? "Loading files" : "No matching files")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                        .accessibilityIdentifier("context.empty")
                } else {
                    List(candidates, id: \.self, selection: $selectedPath) { path in
                        Button {
                            selectedPath = path
                            onSelect(path)
                            dismiss()
                        } label: {
                            HStack(spacing: 8) {
                                Image(systemName: contextFileIcon(for: path))
                                    .font(.caption2.weight(.semibold))
                                    .foregroundStyle(.secondary)
                                    .frame(width: 16, alignment: .center)
                                Text(path)
                                    .font(.system(.body, design: .monospaced))
                                    .foregroundStyle(.primary)
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.vertical, 4)
                            .padding(.horizontal, 8)
                            .background(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .fill(selectedPath == path ? Color.accentColor.opacity(0.14) : Color.clear)
                            )
                        }
                        .buttonStyle(.plain)
                        .tag(path)
                        .accessibilityIdentifier("context.item.\(path)")
                        .accessibilityLabel("Insert reference to \(path)")
                    }
                    .listStyle(.plain)

                    HStack(spacing: 8) {
                        Label("↑/↓", systemImage: "arrow.up.arrow.down")
                        Text("navigate")
                        Text("•")
                        Text("Return")
                        Text("insert")
                        Text("•")
                        Text("Esc")
                        Text("close")
                        Text("•")
                        Text("1-9")
                        Text("quick pick")
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .accessibilityIdentifier("context.keyboard-hints")
                }
            }
            .padding(14)
            .navigationTitle("Insert context")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button("Refresh") {
                        onRefresh()
                    }
                    .accessibilityLabel("Re-index workspace files")
                }
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
            .overlay(alignment: .topLeading) {
                keyboardProxy
            }
            .onAppear {
                syncSelection()
                searchFocused = true
            }
            .onChange(of: candidates) { _, _ in
                syncSelection()
            }
        }
    }

    @ViewBuilder
    private var keyboardProxy: some View {
        VStack(spacing: 0) {
            Button("Insert selected reference") {
                applySelectedCandidate()
            }
            .keyboardShortcut(.defaultAction)
            .opacity(0.001)
            .frame(width: 1, height: 1)
            .disabled(candidates.isEmpty)

            Button("Close context picker") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)
            .opacity(0.001)
            .frame(width: 1, height: 1)

            Button("Previous context candidate") {
                moveSelection(delta: -1)
            }
            .keyboardShortcut(.upArrow, modifiers: [])
            .opacity(0.001)
            .frame(width: 1, height: 1)
            .disabled(candidates.count < 2)

            Button("Next context candidate") {
                moveSelection(delta: 1)
            }
            .keyboardShortcut(.downArrow, modifiers: [])
            .opacity(0.001)
            .frame(width: 1, height: 1)
            .disabled(candidates.count < 2)

            ForEach(Array(candidates.prefix(9).enumerated()), id: \.element) { index, path in
                Button("Insert \(path)") {
                    selectedPath = path
                    applySelectedCandidate()
                }
                .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: [])
                .opacity(0.001)
                .frame(width: 1, height: 1)
            }
        }
        .accessibilityHidden(true)
    }

    private func syncSelection() {
        guard let first = candidates.first else {
            selectedPath = nil
            return
        }

        guard let selectedPath,
              candidates.contains(selectedPath)
        else {
            self.selectedPath = first
            return
        }
    }

    private func moveSelection(delta: Int) {
        guard !candidates.isEmpty else {
            return
        }

        let fallbackIndex = delta >= 0 ? 0 : max(0, candidates.count - 1)
        let currentIndex = selectedIndex ?? fallbackIndex
        let nextIndex = max(0, min(candidates.count - 1, currentIndex + delta))
        selectedPath = candidates[nextIndex]
    }

    private func applySelectedCandidate() {
        guard let selectedCandidate else {
            return
        }

        onSelect(selectedCandidate)
        dismiss()
    }

    private func contextFileIcon(for path: String) -> String {
        let ext = (path as NSString).pathExtension.lowercased()
        switch ext {
        case "swift":
            return "swift"
        case "js", "ts", "jsx", "tsx":
            return "chevron.left.forwardslash.chevron.right"
        case "py":
            return "terminal"
        case "rs":
            return "gearshape"
        case "json", "yaml", "yml", "toml", "plist":
            return "doc.text"
        case "md", "txt", "rst":
            return "doc.richtext"
        case "png", "jpg", "jpeg", "svg", "gif", "webp":
            return "photo"
        default:
            return "doc"
        }
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
    @Binding var sessionGroupingModeRaw: String
    @Binding var sessionRailVisibleLimit: Int
    @Binding var transcriptDensityRaw: String
    @Binding var openDestinationRaw: String
    @Binding var followupModeRaw: String
    @Binding var multilineBehaviorRaw: String
    @Binding var showActivityEvents: Bool
    @Binding var ideContextEnabled: Bool
    @Binding var selectedModel: String
    @Binding var selectedReasoningLevel: String
    @Binding var selectedSandboxMode: String
    @Binding var selectedApprovalPolicy: String
    @Binding var defaultSessionIDERaw: String
    @Binding var preventSleep: Bool
    @Binding var glassWindow: Bool
    let initialCategory: SettingsCategory
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    @State private var selectedCategory: SettingsCategory = .general

    private let modelOptions = WorkflowSettings.modelOptions
    private let reasoningOptions = WorkflowSettings.reasoningOptions
    private let sandboxOptions = WorkflowSettings.sandboxOptions
    private let approvalPolicyOptions = WorkflowSettings.approvalPolicyOptions

    private var availableIDEs: [SessionIDESelection] {
        #if os(macOS)
        IDEAvailability.availableSelections()
        #else
        [.systemDefault]
        #endif
    }

    private var isCompactSettingsLayout: Bool {
        #if os(iOS)
        return horizontalSizeClass == .compact
        #else
        return false
        #endif
    }

    @ViewBuilder
    private var settingsSidebar: some View {
        #if os(macOS)
        List(SettingsCategory.allCases, selection: $selectedCategory) { category in
            Text(category.title)
                .tag(category)
        }
        #else
        List(SettingsCategory.allCases) { category in
            Button {
                selectedCategory = category
            } label: {
                HStack {
                    Text(category.title)
                    Spacer(minLength: 8)
                    if selectedCategory == category {
                        Image(systemName: "checkmark")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .buttonStyle(.plain)
        }
        #endif
    }

    @ViewBuilder
    private var compactCategoryPicker: some View {
        Menu {
            ForEach(SettingsCategory.allCases) { category in
                Button {
                    selectedCategory = category
                } label: {
                    if selectedCategory == category {
                        Label(category.title, systemImage: "checkmark")
                    } else {
                        Text(category.title)
                    }
                }
            }
        } label: {
            HStack(spacing: 8) {
                Text("Category")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                Spacer(minLength: 8)
                Text(selectedCategory.title)
                    .font(.subheadline.weight(.semibold))
                Image(systemName: "chevron.down")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder
    private var settingsDetailContent: some View {
        Text(selectedCategory.title)
            .font(.title2.weight(.semibold))

        switch selectedCategory {
        case .general:
            SettingsInfoCard(text: "Core workflow defaults apply to new turns in this workspace, so you can stay in one place instead of reconfiguring the composer each time.")

            SettingsRow(title: "Open destination", description: "Choose where file actions open by default.") {
                Picker("", selection: $openDestinationRaw) {
                    ForEach(OpenDestination.allCases) { option in
                        Text(option.label).tag(option.rawValue)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: isCompactSettingsLayout ? nil : 220)
                .accessibilityIdentifier("settings.open-destination")
            }

            SettingsRow(title: "Default IDE", description: "IDE used when a thread has no explicit IDE selection.") {
                Picker("", selection: $defaultSessionIDERaw) {
                    ForEach(availableIDEs) { ide in
                        Text(ide.label).tag(ide.rawValue)
                    }
                }
                .frame(width: isCompactSettingsLayout ? nil : 260)
                .accessibilityIdentifier("settings.default-ide")
            }

            SettingsRow(title: "Default model", description: "Model preselected for new turns.") {
                Picker("", selection: $selectedModel) {
                    ForEach(modelOptions, id: \.self) { option in
                        Text(option).tag(option)
                    }
                }
                .frame(width: isCompactSettingsLayout ? nil : 280)
                .accessibilityIdentifier("settings.default-model")
            }

            SettingsRow(title: "Default reasoning", description: "Reasoning effort for new turns.") {
                Picker("", selection: $selectedReasoningLevel) {
                    ForEach(reasoningOptions, id: \.self) { option in
                        Text(option).tag(option)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: isCompactSettingsLayout ? nil : 220)
                .accessibilityIdentifier("settings.default-reasoning")
            }

            SettingsRow(title: "Default sandbox", description: "Execution sandbox mode for command actions.") {
                Picker("", selection: $selectedSandboxMode) {
                    ForEach(sandboxOptions, id: \.self) { option in
                        Text(option).tag(option)
                    }
                }
                .pickerStyle(.menu)
                .frame(width: isCompactSettingsLayout ? nil : 220)
                .accessibilityIdentifier("settings.default-sandbox")
            }

            SettingsRow(title: "Default approvals", description: "How command approvals are requested by default.") {
                Picker("", selection: $selectedApprovalPolicy) {
                    ForEach(approvalPolicyOptions, id: \.self) { option in
                        Text(option).tag(option)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: isCompactSettingsLayout ? nil : 280)
                .accessibilityIdentifier("settings.default-approval")
            }

            SettingsRow(title: "Thread density", description: "Control compactness in the thread list.") {
                Picker("", selection: $threadDensityRaw) {
                    ForEach(ThreadDensity.allCases) { option in
                        Text(option.label).tag(option.rawValue)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: isCompactSettingsLayout ? nil : 240)
            }

            SettingsRow(title: "Thread grouping", description: "Choose how sessions are grouped in the thread rail.") {
                Picker("", selection: $sessionGroupingModeRaw) {
                    ForEach(SessionRailGroupingMode.allCases) { option in
                        Text(option.label).tag(option.rawValue)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: isCompactSettingsLayout ? nil : 260)
            }

            SettingsRow(title: "Visible thread cap", description: "Limit rendered rows for smoother scrolling in large catalogs.") {
                HStack(spacing: 10) {
                    Slider(
                        value: Binding(
                            get: { Double(SessionRailDisplayPolicy.normalizedVisibleLimit(sessionRailVisibleLimit)) },
                            set: { rawValue in
                                let rounded = Int(rawValue.rounded(.toNearestOrAwayFromZero))
                                sessionRailVisibleLimit = SessionRailDisplayPolicy.normalizedVisibleLimit(rounded)
                            }
                        ),
                        in: Double(SessionRailDisplayPolicy.minVisibleLimit)...Double(SessionRailDisplayPolicy.maxVisibleLimit),
                        step: Double(SessionRailDisplayPolicy.visibleLimitStep)
                    )
                    .frame(width: isCompactSettingsLayout ? nil : 220)

                    Text("\(SessionRailDisplayPolicy.normalizedVisibleLimit(sessionRailVisibleLimit))")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                        .frame(width: 44, alignment: .trailing)
                }
            }

            SettingsRow(title: "Transcript density", description: "Adjust message spacing and card width in transcript view.") {
                Picker("", selection: $transcriptDensityRaw) {
                    ForEach(TranscriptDensity.allCases) { option in
                        Text(option.label).tag(option.rawValue)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: isCompactSettingsLayout ? nil : 280)
            }

            SettingsRow(title: "Show activity events", description: "Include reasoning, exec, browser, and coordinator activity in the transcript.") {
                Toggle("", isOn: $showActivityEvents)
                    .labelsHidden()
                    .accessibilityIdentifier("settings.show-activity-events")
            }

            SettingsRow(title: "IDE context", description: "Allow IDE context hints in the composer when building prompts.") {
                Toggle("", isOn: $ideContextEnabled)
                    .labelsHidden()
                    .accessibilityIdentifier("settings.ide-context")
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
                .frame(width: isCompactSettingsLayout ? nil : 260)
            }

            SettingsRow(title: "Follow-up behavior", description: "Choose whether follow-ups queue or steer running turns.") {
                Picker("", selection: $followupModeRaw) {
                    ForEach(FollowupMode.allCases) { option in
                        Text(option.label).tag(option.rawValue)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: isCompactSettingsLayout ? nil : 220)
            }

        case .configuration:
            SettingsRow(title: "Mirror endpoint", description: "WebSocket endpoint used by native clients.") {
                TextField("ws://127.0.0.1:4317/ws", text: $store.endpoint)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: isCompactSettingsLayout ? nil : 320)
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
                .frame(width: isCompactSettingsLayout ? nil : 240)
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
            SettingsInfoCard(text: "MCP servers are managed from the shared Codex configuration for this workspace.")

        case .git:
            SettingsInfoCard(text: "Git defaults, commit style, and review policies come from the repository and Codex agent config.")

        case .environments:
            SettingsInfoCard(text: "Environment profiles are sourced from your current workspace and launch scripts.")

        case .worktrees:
            SettingsInfoCard(text: "Worktree behavior follows your active branch and local repository conventions.")

        case .archivedThreads:
            SettingsInfoCard(text: "Archived threads are available through session history and replay-based restore.")
        }
    }

    var body: some View {
        Group {
            if isCompactSettingsLayout {
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        compactCategoryPicker
                        settingsDetailContent
                    }
                    .padding(16)
                }
            } else {
                HStack(spacing: 0) {
                    settingsSidebar
                        .listStyle(.sidebar)
                        .frame(width: 220)

                    Divider()

                    ScrollView {
                        VStack(alignment: .leading, spacing: 14) {
                            settingsDetailContent
                        }
                        .padding(22)
                    }
                }
            }
        }
        #if os(macOS)
        .background(Color(nsColor: .windowBackgroundColor))
        #else
        .background(Color(uiColor: .systemGroupedBackground))
        #endif
        .onAppear {
            selectedCategory = initialCategory
            selectedModel = WorkflowSettings.normalizedSelection(
                current: selectedModel,
                options: modelOptions,
                fallback: "GPT-5.3-Codex"
            )
            selectedReasoningLevel = WorkflowSettings.normalizedSelection(
                current: selectedReasoningLevel,
                options: reasoningOptions,
                fallback: "High"
            )
            selectedSandboxMode = WorkflowSettings.normalizedSelection(
                current: selectedSandboxMode,
                options: sandboxOptions,
                fallback: "Local"
            )
            selectedApprovalPolicy = WorkflowSettings.normalizedSelection(
                current: selectedApprovalPolicy,
                options: approvalPolicyOptions,
                fallback: "On request"
            )
            sessionGroupingModeRaw = SessionRailGroupingMode(rawValue: sessionGroupingModeRaw)?.rawValue
                ?? SessionRailGroupingMode.repository.rawValue
            sessionRailVisibleLimit = SessionRailDisplayPolicy.normalizedVisibleLimit(sessionRailVisibleLimit)
            defaultSessionIDERaw = SessionIDEPreferences.normalizedDefaultIDE(
                rawDefaultIDE: defaultSessionIDERaw,
                available: availableIDEs
            )
        }
    }
}

private struct SettingsRow<Content: View>: View {
    let title: String
    let description: String
    @ViewBuilder let content: Content

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    private var isCompactSettingsRow: Bool {
        #if os(iOS)
        return horizontalSizeClass == .compact
        #else
        return false
        #endif
    }

    var body: some View {
        Group {
            if isCompactSettingsRow {
                VStack(alignment: .leading, spacing: 10) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(title)
                            .font(.headline)
                        Text(description)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    content
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(14)
                .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            } else {
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
    }
}

private struct SettingsInfoCard: View {
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

private struct TopRoundedPanelShape: Shape {
    let radius: CGFloat

    func path(in rect: CGRect) -> Path {
        let clampedRadius = min(radius, rect.width / 2, rect.height / 2)
        var path = Path()

        path.move(to: CGPoint(x: rect.minX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + clampedRadius))
        path.addQuadCurve(
            to: CGPoint(x: rect.minX + clampedRadius, y: rect.minY),
            control: CGPoint(x: rect.minX, y: rect.minY)
        )
        path.addLine(to: CGPoint(x: rect.maxX - clampedRadius, y: rect.minY))
        path.addQuadCurve(
            to: CGPoint(x: rect.maxX, y: rect.minY + clampedRadius),
            control: CGPoint(x: rect.maxX, y: rect.minY)
        )
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
        path.closeSubpath()

        return path
    }
}

private struct TopRoundedPanelOutlineShape: Shape {
    let radius: CGFloat

    func path(in rect: CGRect) -> Path {
        let clampedRadius = min(radius, rect.width / 2, rect.height / 2)
        var path = Path()

        path.move(to: CGPoint(x: rect.minX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + clampedRadius))
        path.addQuadCurve(
            to: CGPoint(x: rect.minX + clampedRadius, y: rect.minY),
            control: CGPoint(x: rect.minX, y: rect.minY)
        )
        path.addLine(to: CGPoint(x: rect.maxX - clampedRadius, y: rect.minY))
        path.addQuadCurve(
            to: CGPoint(x: rect.maxX, y: rect.minY + clampedRadius),
            control: CGPoint(x: rect.maxX, y: rect.minY)
        )
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))

        return path
    }
}
