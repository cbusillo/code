import SwiftUI

#if os(macOS)
import AppKit
#endif

#if os(iOS)
import UIKit
#endif

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
    @AppStorage("code_native_open_destination") private var openDestinationRaw = OpenDestination.finder.rawValue
    @AppStorage("code_native_followup_mode") private var followupModeRaw = FollowupMode.steer.rawValue
    @AppStorage("code_native_multiline_behavior") private var multilineBehaviorRaw = MultilineBehavior.cmdEnter.rawValue
    @AppStorage("code_native_prevent_sleep") private var preventSleep = false
    @AppStorage("code_native_glass_window") private var glassWindow = true
    @AppStorage("code_native_auto_speak") private var autoSpeakAssistant = true
    @AppStorage("code_native_auto_submit_voice") private var autoSubmitVoice = true
    @AppStorage("code_native_selected_model") private var selectedModel = "GPT-5.3-Codex"
    @AppStorage("code_native_reasoning_level") private var selectedReasoningLevel = "High"
    @AppStorage("code_native_sandbox_mode") private var selectedSandboxMode = "Local"
    @AppStorage("code_native_approval_policy") private var selectedApprovalPolicy = "On request"

    @State private var lastSpokenItemID: String?
    @State private var showSettings = false
    @State private var settingsCategory: SettingsCategory = .general
    @State private var showThreadPicker = false
    @State private var showConnectionPopover = false
    @State private var ideContextEnabled = true
    @State private var activeTranscriptItemID: String?
    @FocusState private var composerIsFocused: Bool

    private let transcriptBottomAnchor = "transcript.bottom"
    private let modelOptions = ["GPT-5.3-Codex", "GPT-5.2", "Claude Sonnet 4.5"]
    private let reasoningOptions = ["Low", "Medium", "High"]
    private let sandboxOptions = ["Local", "Workspace write", "Read-only"]
    private let approvalPolicyOptions = ["On request", "On failure", "Never"]

    private var canSendTurns: Bool {
        store.connectionState == .connected && store.selectedSession != nil
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
        isCompactPhoneLayout ? 4 : 3
    }

    private var welcomePromptCardMinHeight: CGFloat {
        isCompactPhoneLayout ? 98 : 112
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
        let grouped = Dictionary(grouping: visibleSessions) { session in
            let path = URL(fileURLWithPath: session.cwd)
            let repo = path.lastPathComponent
            return repo.isEmpty ? "workspace" : repo
        }

        return grouped
            .map { repoName, sessions in
                let sortedSessions = sessions.sorted(by: { sessionActivityUnixMs($0) > sessionActivityUnixMs($1) })
                return RepoSessionGroup(
                    repoName: repoName,
                    sessions: sortedSessions,
                    latestActivityUnixMs: sortedSessions.first.map(sessionActivityUnixMs) ?? 0
                )
            }
            .sorted(by: {
                if $0.latestActivityUnixMs == $1.latestActivityUnixMs {
                    return $0.repoName.localizedCaseInsensitiveCompare($1.repoName) == .orderedAscending
                }
                return $0.latestActivityUnixMs > $1.latestActivityUnixMs
            })
    }

    private var visibleSessions: [SessionSummary] {
        store.sessions.filter { !isHiddenReviewSession($0) }
    }

    private var hiddenReviewCountByRepo: [String: Int] {
        store.sessions.reduce(into: [String: Int]()) { result, session in
            guard isHiddenReviewSession(session) else {
                return
            }

            let key = linkedRepoNameForHiddenReview(session)
            result[key, default: 0] += 1
        }
    }

    private var totalHiddenReviewCount: Int {
        hiddenReviewCountByRepo.values.reduce(0, +)
    }

    private var sessionTitleCounts: [String: Int] {
        visibleSessions.reduce(into: [String: Int]()) { counts, session in
            counts[sessionDisplayTitle(for: session), default: 0] += 1
        }
    }

    private var sessionCountByRepoName: [String: Int] {
        visibleSessions.reduce(into: [String: Int]()) { counts, session in
            counts[sessionRepoName(for: session), default: 0] += 1
        }
    }

    private var shouldShowRepoHeaders: Bool {
        repoGroups.count > 1
    }

    private var showsIPadSplitLayout: Bool {
        #if os(iOS)
        return UIDevice.current.userInterfaceIdiom == .pad && horizontalSizeClass == .regular
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
        .sheet(isPresented: $showSettings) {
            #if os(iOS)
            NavigationStack {
                settingsSheetContent
                    .navigationTitle("Settings")
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .topBarTrailing) {
                            Button("Done") {
                                showSettings = false
                            }
                            .accessibilityIdentifier("settings.done")
                        }
                    }
            }
            #else
            settingsSheetContent
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
        .onChange(of: store.selectedSessionItems.last?.id) { _, _ in
            handleAssistantSpeech()
            if let activeTranscriptItemID,
               !transcriptItems.contains(where: { $0.id == activeTranscriptItemID }) {
                self.activeTranscriptItemID = nil
            }
        }
        .onChange(of: store.selectedSessionID) { _, _ in
            activeTranscriptItemID = nil
            #if os(iOS)
            showThreadPicker = false
            #else
            composerIsFocused = true
            #endif
        }
        .onChange(of: store.sessions) { _, _ in
            ensureVisibleSelection()
        }
        #if os(iOS)
        .onChange(of: showsIPadSplitLayout) { _, isSplit in
            if isSplit {
                showThreadPicker = false
            }
        }
        #endif
        .onDisappear {
            _ = voiceInput.stopRecording()
            voiceOutput.stop()
        }
    }

    @ViewBuilder
    private var settingsSheetContent: some View {
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

                ActionRailButton(icon: "arrow.clockwise", title: "Refresh", accessibilityID: "rail.refresh") {
                    Task {
                        await store.refreshSessions()
                    }
                }
                #if os(macOS)
                .keyboardShortcut("r", modifiers: [.command])
                #endif

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
                VStack(alignment: .leading, spacing: 14) {
                    if repoGroups.isEmpty {
                        VStack(alignment: .leading, spacing: 10) {
                            Text("No threads")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.white.opacity(0.9))
                            Text("Start your first local thread to populate the sidebar.")
                                .font(.caption)
                                .foregroundStyle(.secondary)

                            if totalHiddenReviewCount > 0 {
                                Text("\(totalHiddenReviewCount) auto-review threads are hidden.")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }

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
                        HStack(spacing: 8) {
                            Text("Threads")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.white.opacity(0.9))

                            if totalHiddenReviewCount > 0 {
                                Text("\(totalHiddenReviewCount) hidden")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary.opacity(0.9))
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(Color.white.opacity(0.04), in: Capsule(style: .continuous))
                            }

                            Spacer()
                        }

                        ForEach(repoGroups) { group in
                            VStack(alignment: .leading, spacing: 8) {
                                if shouldShowRepoHeaders {
                                    HStack {
                                        Text(group.repoName)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)

                                        Spacer()
                                        Text("\(group.sessions.count)")
                                            .font(.caption2)
                                            .foregroundStyle(.secondary)
                                    }
                                }

                                if group.sessions.isEmpty {
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
                composerPanel
            } else {
                emptyState
                composerPanel
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var topBar: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(store.selectedSession.map(sessionTitle(for:)) ?? "New thread")
                .font(.headline.weight(.semibold))
                .foregroundStyle(.white.opacity(0.95))
                .lineLimit(topBarTitleLineLimit)
                .minimumScaleFactor(0.85)
                .truncationMode(.tail)

            HStack(spacing: 8) {
                if !isCompactPhoneLayout {
                    Text(store.selectedSession.map(sessionSubtitleLine(for:)) ?? "")
                        .font(.caption)
                        .foregroundStyle(.white.opacity(0.62))
                        .lineLimit(1)
                        .truncationMode(.tail)
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
            Button {
                Task {
                    await store.createSession(cwd: nil)
                }
            } label: {
                Label("New thread", systemImage: "square.and.pencil")
            }
            .accessibilityIdentifier("top.quick-actions.new-thread")

            Button {
                Task {
                    await store.refreshSessions()
                }
            } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .accessibilityIdentifier("top.quick-actions.refresh")

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
        } label: {
            Image(systemName: "ellipsis.circle")
                .font(.subheadline.weight(.semibold))
                .frame(width: topBarActionButtonSize, height: topBarActionButtonSize)
                .background(Color.white.opacity(0.06), in: Circle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(.white.opacity(0.9))
        .accessibilityIdentifier("top.quick-actions")

        if !showsIPadSplitLayout {
            TopBarIconButton(
                icon: "list.bullet",
                accessibilityID: "top.threads",
                buttonSize: topBarActionButtonSize
            ) {
                showThreadPicker = true
            }
        }

        TopBarIconButton(
            icon: "gearshape",
            accessibilityID: "top.settings",
            buttonSize: topBarActionButtonSize
        ) {
            presentSettings(.general)
        }
        #else
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

    private var topBarTitleLineLimit: Int {
        #if os(iOS)
        return isCompactPhoneLayout ? 2 : 1
        #else
        return 1
        #endif
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
                    VStack(spacing: 12) {
                        if transcriptItems.isEmpty {
                            welcomePanel(for: session)
                        } else {
                            ForEach(transcriptItems) { item in
                                transcriptRow(item: item)
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
                    scrollTranscriptToBottom(proxy: proxy, animated: false)
                }
                .onChange(of: store.selectedSessionItems.last?.id) { _, _ in
                    scrollTranscriptToBottom(proxy: proxy, animated: true)
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
            onActivate: {
                activeTranscriptItemID = item.id
            },
            onApproval: { decision in
                handleApproval(item: item, decision: decision)
            }
        )
        .frame(minWidth: widthMin)
        .frame(maxWidth: widthCap, alignment: item.prefersTrailingBubble ? .trailing : .leading)
        .fixedSize(horizontal: shouldFitContentWidth, vertical: false)

        return HStack(spacing: 0) {
            if item.cardStyle == .assistant {
                AssistantTranscriptLine(text: item.body)
                    .frame(maxWidth: 760, alignment: .leading)
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
        return showsIPadSplitLayout ? 18 : 12
        #else
        return 26
        #endif
    }

    private var transcriptBubbleGutter: CGFloat {
        #if os(iOS)
        return showsIPadSplitLayout ? 34 : 10
        #else
        return 72
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
        let visible = store.selectedSessionItems.filter { !$0.shouldHideFromTranscript && !$0.isReplayOmittedNotice }
        let replayPruned = pruneReplayHistoryCardsWhenRedundant(in: visible)

        return dedupeAssistantMessagesWithinTurn(
            in: collapseConsecutiveReasoningCards(
                in: dedupeObsoleteBackgroundEvents(
                    in: collapseTokenUsageBursts(
                        in: removeRedundantPatchSummaries(
                            from: dedupeObsoletePatchApplyCards(in: dedupeObsoleteDiffs(in: replayPruned))
                        )
                    )
                )
            )
        )
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
        VStack(spacing: 22) {
            Spacer(minLength: 34)

            Image(systemName: "sparkles")
                .font(.title2.weight(.semibold))
                .foregroundStyle(.white.opacity(0.9))
                .frame(width: 52, height: 52)
                .background(Color.white.opacity(0.08), in: Circle())

            VStack(spacing: 6) {
                Text("Let's build")
                    .font(.system(size: welcomePrimaryFontSize, weight: .semibold))
                    .foregroundStyle(.white.opacity(0.95))
                Text(sessionDisplayTitle(for: session))
                    .font(.system(size: welcomeSecondaryFontSize, weight: .semibold))
                    .foregroundStyle(.white.opacity(0.55))
                    .lineLimit(welcomeSecondaryLineLimit)
                    .minimumScaleFactor(0.75)
                    .multilineTextAlignment(.center)
            }

            welcomePromptSection

            Spacer(minLength: 24)
        }
        .padding(.top, isCompactPhoneLayout ? 12 : 36)
        .frame(maxWidth: .infinity)
    }

    @ViewBuilder
    private var welcomePromptSection: some View {
        if shouldUsePromptScroller {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    ForEach(quickStartPrompts, id: \.self) { prompt in
                        QuickPromptCard(
                            prompt: prompt,
                            lineLimit: welcomePromptCardLineLimit,
                            minHeight: welcomePromptCardMinHeight
                        ) {
                            store.composerText = prompt
                        }
                        .frame(width: 224)
                    }
                }
                .padding(.horizontal, 2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        } else {
            HStack(spacing: 12) {
                ForEach(quickStartPrompts, id: \.self) { prompt in
                    QuickPromptCard(
                        prompt: prompt,
                        lineLimit: welcomePromptCardLineLimit,
                        minHeight: welcomePromptCardMinHeight
                    ) {
                        store.composerText = prompt
                    }
                }
            }
            .frame(maxWidth: 900)
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
                        .frame(height: composerEditorHeight)
                        .focused($composerIsFocused)
                        .padding(.horizontal, 12)
                        .padding(.top, 8)
                        .accessibilityIdentifier("composer.input")

                    if store.composerText.isEmpty {
                        Text("Ask for follow-up changes")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 18)
                            .padding(.top, 14)
                            .allowsHitTesting(false)
                    }
                }

                composerControlRows
            }
            .background(Color.white.opacity(0.045), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .stroke(Color.white.opacity(0.09), lineWidth: 1)
            )
        }
        .padding(.horizontal, composerHorizontalPadding)
        .padding(.bottom, composerBottomPadding)
    }

    private var composerHorizontalPadding: CGFloat {
        #if os(iOS)
        return showsIPadSplitLayout ? 16 : 22
        #else
        return 22
        #endif
    }

    private var composerBottomPadding: CGFloat {
        #if os(iOS)
        return 0
        #else
        return 18
        #endif
    }

    private var composerControlRows: some View {
        #if os(iOS)
        VStack(spacing: 0) {
            HStack(spacing: 8) {
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

                Spacer(minLength: 10)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.top, 8)
            .padding(.bottom, 4)

            HStack(spacing: 8) {
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
                .disabled(!canSendTurns)
                .accessibilityIdentifier("composer.mic-toggle")

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
                #if os(macOS)
                .keyboardShortcut(".", modifiers: [.command])
                #endif

                Spacer(minLength: 6)

                VoiceStatusBadge(label: voiceStateLabel, isActive: voiceInput.isRecording)
                    .font(.caption2)

                Button {
                    submitComposerAction()
                } label: {
                    Image(systemName: "arrow.up")
                        .font(.caption.weight(.bold))
                        .foregroundStyle(.black)
                }
                .buttonStyle(.plain)
                .frame(width: 28, height: 28)
                .background(
                    canSubmit ? Color.white : Color.white.opacity(0.35),
                    in: Circle()
                )
                .disabled(!canSubmit)
                .accessibilityIdentifier("composer.send")
                #if os(macOS)
                .keyboardShortcut(.return, modifiers: [.command])
                #endif
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.top, 2)
            .padding(.bottom, 8)
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
                .disabled(!canSendTurns)
                .accessibilityIdentifier("composer.mic-toggle")

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
                #if os(macOS)
                .keyboardShortcut(".", modifiers: [.command])
                #endif

                Button {
                    submitComposerAction()
                } label: {
                    Image(systemName: "arrow.up")
                        .font(.caption.weight(.bold))
                        .foregroundStyle(.black)
                }
                .buttonStyle(.plain)
                .frame(width: 28, height: 28)
                .background(
                    canSubmit ? Color.white : Color.white.opacity(0.35),
                    in: Circle()
                )
                .disabled(!canSubmit)
                .accessibilityIdentifier("composer.send")
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

    private func presentSettings(_ category: SettingsCategory) {
        settingsCategory = category
        showSettings = true
    }

    private func submitComposerAction() {
        Task {
            await store.submitComposer()
        }
    }

    private func interruptTurnAction() {
        Task {
            await store.interruptTurn()
        }
    }

    private var composerEditorHeight: CGFloat {
        let text = store.composerText
        guard !text.isEmpty else {
            return 48
        }

        let newlineCount = text.reduce(into: 1) { count, character in
            if character == "\n" {
                count += 1
            }
        }
        let wrappedLineEstimate = max(1, text.count / 72)
        let lines = max(newlineCount, wrappedLineEstimate)
        let height = CGFloat(lines * 20 + 20)
        return min(176, max(48, height))
    }

    private var selectedSessionRepoLabel: String {
        guard let selectedSession = store.selectedSession else {
            return "workspace"
        }
        return sessionRepoName(for: selectedSession)
    }

    private func sessionTitle(for session: SessionSummary) -> String {
        sessionSidebarTitle(for: session)
    }

    private func ensureVisibleSelection() {
        if let selectedSession = store.selectedSession,
           !isHiddenReviewSession(selectedSession) {
            return
        }

        store.selectedSessionID = visibleSessions.last?.id
    }

    private func isHiddenReviewSession(_ session: SessionSummary) -> Bool {
        let normalized = session.cwd.lowercased()
        return normalized.contains("/.code/working/") && normalized.contains("/branches/auto-review")
    }

    private func linkedRepoNameForHiddenReview(_ session: SessionSummary) -> String {
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
        if let summaryTitle = sessionSummaryTitle(session) {
            return summaryTitle
        }

        if let threadTitle = sessionThreadTitle(for: session) {
            return threadTitle
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
        let repoName = URL(fileURLWithPath: session.cwd).lastPathComponent
        let folder = repoName.isEmpty ? "workspace" : repoName
        return folder
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

        guard !title.isEmpty else {
            return nil
        }

        if title.hasPrefix("[image:") {
            return nil
        }

        if title.lowercased().hasPrefix("context: repo:") {
            let pathPart = title.dropFirst("Context: Repo:".count).trimmingCharacters(in: .whitespaces)
            let repo = URL(fileURLWithPath: pathPart).lastPathComponent
            if !repo.isEmpty {
                title = "Context: \(repo)"
            }
        }

        let maxLength = 46
        if title.count > maxLength {
            let end = title.index(title.startIndex, offsetBy: maxLength)
            title = "\(title[..<end])…"
        }

        return title
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
        #if os(iOS)
        if item.isPatchApplyEndEvent || item.isTokenCountEvent || item.isBackgroundEvent {
            return 360
        }

        if item.prefersTrailingBubble {
            let estimated = CGFloat(item.body.count) * 5.4 + 64
            return min(280, max(130, estimated))
        }

        switch item.cardStyle {
        case .tool, .approval:
            return 360
        case .reasoning, .composer, .system, .defaultStyle, .assistant, .user:
            return 350
        }
        #else
        if item.isPatchApplyEndEvent {
            return 620
        }

        if item.isTokenCountEvent {
            return 760
        }

        if item.isBackgroundEvent {
            return 760
        }

        if item.prefersTrailingBubble {
            let estimated = CGFloat(item.body.count) * 6.4 + 76
            return min(520, max(170, estimated))
        }

        switch item.cardStyle {
        case .tool, .approval:
            return 700
        case .reasoning, .composer, .system, .defaultStyle, .assistant, .user:
            return 620
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
        max(session.lastEventAtUnixMs, session.createdAtUnixMs)
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
        case .finder, .editor:
            NSWorkspace.shared.open(url)
        }
        #else
        switch destination {
        case .finder, .editor:
            openURL(url)
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
    var id: String { repoName }
    let repoName: String
    let sessions: [SessionSummary]
    let latestActivityUnixMs: UInt64
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
                Text(store.statusLine)
                    .font(.caption)
                    .foregroundStyle(.secondary)
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

private struct AssistantTranscriptLine: View {
    let text: String

    private var renderedText: Text {
        if let attributed = try? AttributedString(
            markdown: text,
            options: AttributedString.MarkdownParsingOptions(
                interpretedSyntax: .full,
                failurePolicy: .returnPartiallyParsedIfPossible
            )
        ) {
            return Text(attributed)
        }

        return Text(text)
    }

    var body: some View {
        renderedText
            .font(.body)
            .foregroundStyle(.white.opacity(0.93))
            .lineSpacing(4)
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 4)
            .padding(.vertical, 2)
            .frame(maxWidth: .infinity, alignment: .leading)
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

private struct TranscriptCard: View {
    let item: SessionStreamItem
    let isActive: Bool
    let onActivate: () -> Void
    let onApproval: (ApprovalDecisionChoice) -> Void

    @State private var selectedDecision: ApprovalDecisionChoice = .approved
    @State private var isExpanded = false
    @State private var diffExpansionSteps = 0

    private var cardBackground: Color {
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
            return Color.yellow.opacity(0.10)
        case .composer:
            return Color.gray.opacity(0.07)
        case .system:
            return Color.gray.opacity(0.08)
        case .defaultStyle:
            return Color.white.opacity(0.05)
        }
    }

    private var cardBorder: Color {
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
            return Color.yellow.opacity(0.22)
        case .composer:
            return Color.gray.opacity(0.18)
        case .system:
            return Color.gray.opacity(0.20)
        case .defaultStyle:
            return Color.white.opacity(0.14)
        }
    }

    private var usesMonospacedBody: Bool {
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
        item.isPatchApplyEndEvent || item.isTokenCountEvent || item.isBackgroundEvent
    }

    private var showsMetaHeader: Bool {
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

    private var shouldDrawBorder: Bool {
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

    private var collapsedBodyLimit: Int {
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

    private var totalDiffLines: Int {
        guard let diff = item.turnDiffText else {
            return 0
        }

        let lines = diff.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        let filtered = lines.filter { !BeautifulDiffPreview.isMetadataLine($0) }
        return (filtered.isEmpty ? lines : filtered).count
    }

    private var baseDiffLineLimit: Int {
        isActive ? 12 : 7
    }

    private var diffExpandChunkSize: Int {
        20
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
        case .approval, .composer, .assistant, .user:
            return false
        }
    }

    private var bodyLineLimit: Int? {
        guard shouldClampBodyLines else {
            return nil
        }
        return isActive ? 18 : 7
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if showsMetaHeader {
                HStack {
                    Text(item.title)
                        .font(.caption.weight(.semibold))
                        .textCase(.uppercase)
                        .foregroundStyle(.secondary)
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

                preview
            } else if item.isBackgroundEvent {
                let lines = item.body.components(separatedBy: "\n").filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
                let headline = lines.first ?? "Background event"
                let details = Array(lines.dropFirst().prefix(2)).joined(separator: "\n")

                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 7) {
                        Image(systemName: "checkmark.seal.fill")
                            .font(.caption)
                            .foregroundStyle(Color.green.opacity(0.9))
                        Text(headline)
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.white.opacity(0.95))
                    }

                    if !details.isEmpty {
                        Text(details)
                            .font(.caption)
                            .foregroundStyle(.white.opacity(0.78))
                            .lineSpacing(2)
                    }
                }
            } else if let exec = item.execCommandInfo {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        Text("Ran command")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)

                        Spacer()

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
                    if !outputPreview.isEmpty {
                        Text(outputPreview)
                            .font(.caption.monospaced())
                            .foregroundStyle(.white.opacity(0.86))
                            .lineSpacing(2)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 7)
                            .background(Color.black.opacity(0.18), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
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
            } else {
                Text(displayedBody)
                    .font(usesMonospacedBody ? .body.monospaced() : (usesCompactBodyText ? .subheadline : .body))
                    .foregroundStyle(.white.opacity(0.93))
                    .lineSpacing(3)
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
                                ? Color.white.opacity(0.10)
                                : Color.white.opacity(0.04),
                                in: RoundedRectangle(cornerRadius: 10, style: .continuous)
                            )
                        }
                        .buttonStyle(.plain)
                    }

                    HStack(spacing: 10) {
                        Button("Skip") {
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(.secondary)

                        Spacer()

                        Button("Submit") {
                            onApproval(selectedDecision)
                        }
                        .buttonStyle(.borderedProminent)
                    }
                    .padding(.top, 4)
                }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(cardBackground)
        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay {
            if shouldDrawBorder {
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .stroke(cardBorder, lineWidth: 1)
            }
        }
        .overlay {
            if isActive {
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .stroke(Color.white.opacity(0.16), lineWidth: 1)
            }
        }
        .onTapGesture {
            onActivate()
        }
        .onChange(of: isActive) { _, active in
            if !active {
                diffExpansionSteps = 0
                isExpanded = false
            }
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
    let initialCategory: SettingsCategory

    @State private var selectedCategory: SettingsCategory = .general

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

    var body: some View {
        HStack(spacing: 0) {
            settingsSidebar
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
                .padding(22)
            }
        }
        #if os(macOS)
        .background(Color(nsColor: .windowBackgroundColor))
        #else
        .background(Color(uiColor: .systemGroupedBackground))
        #endif
        .onAppear {
            selectedCategory = initialCategory
        }
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
