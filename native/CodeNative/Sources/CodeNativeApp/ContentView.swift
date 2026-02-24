import SwiftUI
import CoreImage
import CoreImage.CIFilterBuiltins

#if os(macOS)
import AppKit
#endif

#if os(iOS)
import UIKit
import VisionKit
#endif

#if os(iOS)
// iOS demo target compiles shared UI sources without the macOS runtime supervisor.
// Provide a lightweight pairing model so shared settings UI still builds.
struct CompanionPairingEntry: Equatable, Identifiable {
    let id: String
    var label: String
    var sessionToken: String
    let createdAtUnixMs: UInt64
    var expiresAtUnixMs: UInt64
    var revokedAtUnixMs: UInt64?

    var isRevoked: Bool {
        revokedAtUnixMs != nil
    }

    func isExpired(referenceUnixMs: UInt64) -> Bool {
        referenceUnixMs >= expiresAtUnixMs
    }

    var displayLabel: String {
        if !label.isEmpty {
            return label
        }

        let shortID = String(id.prefix(8))
        if !shortID.isEmpty {
            return "Device \(shortID)"
        }

        return "Device"
    }
}

// The iOS demo excludes CodeNativeApp.swift, so define shared helper types here.
struct ImportedProfileState: Equatable {
    let sourcePath: String
    let importedAt: Date
}

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
            return "Companion is unreachable. This iOS app has no local backend; pair with a Mac companion and reconnect."
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
#endif

private func currentUnixTimeMilliseconds() -> UInt64 {
    UInt64(Date().timeIntervalSince1970 * 1_000)
}

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
    let companionPairingEntries: [CompanionPairingEntry]
    let canRotateCompanionToken: Bool
    let rotateCompanionToken: (() -> Void)?
    let createCompanionPairing: ((String?) -> Void)?
    let revokeCompanionPairing: ((String) -> Void)?
    let restoreCompanionPairing: ((String) -> Void)?
    let deleteCompanionPairing: ((String) -> Void)?
    let importedProfileState: ImportedProfileState?
    let importCLIProfile: ((URL) throws -> ImportedProfileState)?
    let clearImportedCLIProfile: (() -> Void)?
    @Environment(\.openURL) private var openURL
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    init(
        store: SessionMirrorStore,
        companionPairingEntries: [CompanionPairingEntry] = [],
        canRotateCompanionToken: Bool = false,
        rotateCompanionToken: (() -> Void)? = nil,
        createCompanionPairing: ((String?) -> Void)? = nil,
        revokeCompanionPairing: ((String) -> Void)? = nil,
        restoreCompanionPairing: ((String) -> Void)? = nil,
        deleteCompanionPairing: ((String) -> Void)? = nil,
        importedProfileState: ImportedProfileState? = nil,
        importCLIProfile: ((URL) throws -> ImportedProfileState)? = nil,
        clearImportedCLIProfile: (() -> Void)? = nil
    ) {
        self.store = store
        self.companionPairingEntries = companionPairingEntries
        self.canRotateCompanionToken = canRotateCompanionToken
        self.rotateCompanionToken = rotateCompanionToken
        self.createCompanionPairing = createCompanionPairing
        self.revokeCompanionPairing = revokeCompanionPairing
        self.restoreCompanionPairing = restoreCompanionPairing
        self.deleteCompanionPairing = deleteCompanionPairing
        self.importedProfileState = importedProfileState
        self.importCLIProfile = importCLIProfile
        self.clearImportedCLIProfile = clearImportedCLIProfile
    }

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
    #if os(iOS)
    @AppStorage("code_native_multiline_behavior") private var multilineBehaviorRaw = MultilineBehavior.enter.rawValue
    #else
    @AppStorage("code_native_multiline_behavior") private var multilineBehaviorRaw = MultilineBehavior.cmdEnter.rawValue
    #endif
    @AppStorage("code_native_prevent_sleep") private var preventSleep = false
    @AppStorage("code_native_glass_window") private var glassWindow = true
    @AppStorage("code_native_auto_speak") private var autoSpeakAssistant = false
    @AppStorage("code_native_voice_identifier") private var preferredVoiceIdentifier = ""
    @AppStorage("code_native_voice_rate") private var voicePlaybackRate = 0.46
    @AppStorage("code_native_auto_submit_voice") private var autoSubmitVoice = true
    @AppStorage("code_native_show_activity_events") private var showActivityEvents = true
    @AppStorage("code_native_ide_context_enabled") private var ideContextEnabled = true
    @AppStorage("code_native_selected_model") private var selectedModel = WorkflowSettings.defaultModelLabel
    @AppStorage("code_native_reasoning_level") private var selectedReasoningLevel = WorkflowSettings.defaultReasoningLabel
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
    @State private var showCompanionConnectAssistant = false
    @State private var activeTranscriptItemID: String?
    @State private var cachedTranscriptItems: [SessionStreamItem] = []
    @State private var taskActivityByStartItemID: [String: [String]] = [:]
    @State private var transcriptCacheBySessionID: [UUID: TranscriptSessionCacheEntry] = [:]
    @State private var transcriptCacheUsageOrder: [UUID] = []
    @State private var transcriptRefreshTask: Task<Void, Never>?
    @State private var pendingPrependAnchorItemID: String?
    @State private var pendingBottomScrollAfterThreadSwitch = false
    @State private var pendingBottomPinPassesAfterThreadSwitch = 0
    @State private var transcriptIsNearBottom = true
    @State private var transcriptUserHasScrolled = false
    @State private var composerDraft = ""
    @State private var composerMeasuredHeight: CGFloat = 34
    @State private var showSlashCommandLauncher = false
    @State private var slashCommandQuery = ""
    @State private var slashLauncherOpenedByTyping = false
    @State private var showContextPicker = false
    @State private var contextPickerQuery = ""
    @State private var selectedInlineContextPath: String?
    @State private var ideOpenFailureMessage: String?
    @State private var indexedContextRootPath: String?
    @State private var indexedContextFilePaths: [String] = []
    @State private var contextIndexLoading = false
    @FocusState private var composerIsFocused: Bool

    private let transcriptBottomAnchor = "transcript.bottom"
    private let transcriptScrollCoordinateSpaceName = "transcript.scroll"
    private let transcriptSessionCacheLimit = 6
    private let transcriptRefreshDebounceNanoseconds: UInt64 = 45_000_000
    private let sandboxOptions = WorkflowSettings.sandboxOptions
    private let approvalPolicyOptions = WorkflowSettings.approvalPolicyOptions

    private struct ModelPickerOption: Hashable {
        let model: String
        let label: String
        let defaultReasoningEffort: String
        let supportedReasoningEfforts: [String]
    }

    private struct ReasoningPickerOption: Hashable {
        let effort: String
        let label: String
    }

    private var modelPickerOptions: [ModelPickerOption] {
        availableModelPickerOptions()
    }

    private var modelOptions: [String] {
        modelPickerOptions.map(\.label)
    }

    private var selectedModelPickerOption: ModelPickerOption? {
        modelPickerOptions.first(where: {
            $0.label.caseInsensitiveCompare(selectedModel) == .orderedSame
                || $0.model.caseInsensitiveCompare(selectedModel) == .orderedSame
        })
    }

    private var reasoningPickerOptions: [ReasoningPickerOption] {
        availableReasoningPickerOptions(for: selectedModelPickerOption)
    }

    private var reasoningOptions: [String] {
        reasoningPickerOptions.map(\.label)
    }

    private var selectedMultilineBehavior: MultilineBehavior {
        MultilineBehavior(rawValue: multilineBehaviorRaw) ?? .enter
    }

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

    private var voicePlaybackIconName: String {
        autoSpeakAssistant ? "speaker.wave.2.fill" : "speaker.slash.fill"
    }

    private var voicePlaybackAccessibilityLabel: String {
        autoSpeakAssistant ? "Disable voice playback" : "Enable voice playback"
    }

    private var voicePlaybackAccessibilityHint: String {
        autoSpeakAssistant
            ? "Turns off spoken assistant responses."
            : "Turns on spoken assistant responses."
    }

    private var selectedVoiceIdentifier: String? {
        let trimmed = preferredVoiceIdentifier.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
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
        let compactModel = selectedModelDisplayLabel
            .replacingOccurrences(of: "-Codex", with: "")
            .replacingOccurrences(of: "-Mini", with: "")
        return "\(compactModel) · \(selectedReasoningDisplayLabel)"
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

    private var hasPendingCompanionApproval: Bool {
        for item in store.selectedSessionItems.reversed() {
            guard item.type == "core_event",
                  let payloadType = item.event?.payload?.typeHint
            else {
                continue
            }

            if payloadType == "exec_approval_request"
                || payloadType == "apply_patch_approval_request"
                || payloadType == "request_user_input"
            {
                return true
            }

            if payloadType == "agent_message"
                || payloadType == "user_message"
                || payloadType == "turn_aborted"
                || payloadType == "exec_command_begin"
                || payloadType == "patch_apply_begin"
                || payloadType == "user_input_answer"
            {
                return false
            }
        }

        return false
    }

    private var companionConnectionState: CompanionConnectionState {
        CompanionConnectionState.resolve(
            connectionState: store.connectionState,
            statusLine: store.statusLine,
            lastError: store.lastError,
            hasPendingApproval: hasPendingCompanionApproval
        )
    }

    private var companionConnectionColor: Color {
        switch companionConnectionState {
        case .discovering:
            return Color.blue.opacity(0.88)
        case .pairRequired:
            return Color.orange.opacity(0.9)
        case .approvalPending:
            return Color.yellow.opacity(0.92)
        case .connected:
            return Color.green.opacity(0.85)
        case .reconnecting:
            return Color.orange.opacity(0.9)
        case .offline:
            return Color.red.opacity(0.86)
        }
    }

    private var companionConnectionIcon: String {
        switch companionConnectionState {
        case .discovering:
            return "dot.radiowaves.left.and.right"
        case .pairRequired:
            return "qrcode"
        case .approvalPending:
            return "hand.raised"
        case .connected:
            return "link"
        case .reconnecting:
            return "arrow.clockwise"
        case .offline:
            return "wifi.slash"
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
        .sheet(isPresented: $showCompanionConnectAssistant) {
            CompanionConnectAssistantSheet(store: store)
        }
        #endif
        .task {
            #if os(iOS)
            // iOS is companion-only; avoid reconnect loops before the user pairs.
            if store.connectionState == .disconnected,
               store.companionSessionToken != nil {
                await store.connect()
            }
            #else
            if store.connectionState == .disconnected {
                await store.connect()
            }
            #endif
        }
        .onChange(of: selectedSessionTranscriptSourceKey) { _, _ in
            scheduleTranscriptCacheRefresh(
                delayNanoseconds: transcriptRefreshDebounceNanoseconds,
                preserveBottomPin: true
            )
            handleAssistantSpeech()
            enforceVoiceCapturePolicy(clearTranscript: true)
            if let activeTranscriptItemID,
               !transcriptItems.contains(where: { $0.id == activeTranscriptItemID }) {
                self.activeTranscriptItemID = nil
            }
        }
        .onChange(of: autoSpeakAssistant) { _, isEnabled in
            if !isEnabled {
                voiceOutput.stop()
            }
        }
        .onChange(of: preferredVoiceIdentifier) { _, _ in
            voiceOutput.stop()
        }
        .onChange(of: voicePlaybackRate) { _, _ in
            voiceOutput.stop()
        }
        .onChange(of: store.selectedSessionID) { _, _ in
            enforceIDEContextEnabled()
            if voiceInput.isRecording {
                stopVoiceCapture(shouldSubmit: false, clearTranscript: true)
            }
            voiceInteractionNotice = nil
            activeTranscriptItemID = nil
            transcriptIsNearBottom = true
            transcriptUserHasScrolled = false
            pendingBottomScrollAfterThreadSwitch = true
            pendingBottomPinPassesAfterThreadSwitch = 3
            composerDraft = store.composerText
            scheduleTranscriptCacheRefresh(delayNanoseconds: 0, preserveBottomPin: false)
            ensureContextIndexLoaded()
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
            if ideContextEnabled,
               ComposerContextReferenceFormatter.trailingMentionMatch(in: newValue) != nil {
                ensureContextIndexLoaded()
            }
            syncInlineContextSelection()
            syncSlashCommandLauncher(with: newValue)
        }
        .onChange(of: showSlashCommandLauncher) { _, isPresented in
            if !isPresented {
                slashLauncherOpenedByTyping = false
            }
        }
        .onChange(of: ideContextEnabled) { _, isEnabled in
            if isEnabled {
                return
            }

            indexedContextRootPath = nil
            indexedContextFilePaths = []
            contextIndexLoading = false
            selectedInlineContextPath = nil
            contextPickerQuery = ""
            showContextPicker = false
        }
        .onChange(of: displayedInlineContextPaths) { _, _ in
            syncInlineContextSelection()
        }
        .onChange(of: store.sessions) { _, _ in
            pruneSessionIDEPreferences()
            ensureVisibleSelection()
        }
        .onChange(of: showActivityEvents) { _, _ in
            clearTranscriptSessionCaches()
            scheduleTranscriptCacheRefresh(delayNanoseconds: 0, preserveBottomPin: false)
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
            transcriptRefreshTask?.cancel()
            transcriptRefreshTask = nil
            stopVoiceCapture(shouldSubmit: false, clearTranscript: true)
            voiceOutput.stop()
        }
        #if os(macOS)
        .onAppear {
            enforceIDEContextEnabled()
            composerDraft = store.composerText
            scheduleTranscriptCacheRefresh(delayNanoseconds: 0, preserveBottomPin: false)
            ensureContextIndexLoaded()
            normalizeWorkflowSettings()
            pruneSessionIDEPreferences()
            focusComposerEditor(forceActivateApp: true)
        }
        #else
        .onAppear {
            enforceIDEContextEnabled()
            composerDraft = store.composerText
            scheduleTranscriptCacheRefresh(delayNanoseconds: 0, preserveBottomPin: false)
            ensureContextIndexLoaded()
            normalizeWorkflowSettings()
            pruneSessionIDEPreferences()
        }
        #endif
    }

    @ViewBuilder
    private var settingsSheetContent: some View {
        NativeSettingsView(
            store: store,
            companionPairingEntries: companionPairingEntries,
            canRotateCompanionToken: canRotateCompanionToken,
            rotateCompanionToken: rotateCompanionToken,
            createCompanionPairing: createCompanionPairing,
            revokeCompanionPairing: revokeCompanionPairing,
            restoreCompanionPairing: restoreCompanionPairing,
            deleteCompanionPairing: deleteCompanionPairing,
            importedProfileState: importedProfileState,
            importCLIProfile: importCLIProfile,
            clearImportedCLIProfile: clearImportedCLIProfile,
            autoSpeakAssistant: $autoSpeakAssistant,
            preferredVoiceIdentifier: $preferredVoiceIdentifier,
            voicePlaybackRate: $voicePlaybackRate,
            autoSubmitVoice: $autoSubmitVoice,
            previewVoiceSample: previewVoiceSample,
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
            modelOptions: modelOptions,
            reasoningOptions: reasoningOptions,
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
                        await createSessionWithCurrentWorkflowSettings()
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
                                        await createSessionWithCurrentWorkflowSettings()
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

            if let toolUsageBannerState {
                if toolUsageBannerState.phase == .active {
                    toolUsageBanner(state: toolUsageBannerState)
                        .padding(.horizontal, isCompactPhoneLayout ? 12 : 18)
                        .padding(.top, 6)
                        .padding(.bottom, usesBottomInsetComposer ? 8 : 6)
                }
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

                #if os(iOS)
                if true {
                    statusChip
                }
                #else
                if store.connectionState != .connected {
                    statusChip
                }
                #endif

                topBarActions
            }
        }
        .padding(.horizontal, isCompactPhoneLayout ? 14 : 18)
        .padding(.vertical, isCompactPhoneLayout ? 8 : 10)
        .background(Color.black.opacity(0.24))
    }

    private func toolUsageBanner(state: ToolUsageBannerState) -> some View {
        HStack(spacing: 10) {
            Image(systemName: state.icon)
                .font(.caption.weight(.semibold))
                .foregroundStyle(state.tint)

            VStack(alignment: .leading, spacing: 2) {
                Text(state.title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.92))
                    .lineLimit(1)

                if let detail = state.detail,
                   !detail.isEmpty {
                    Text(detail)
                        .font(.caption2.monospaced())
                        .foregroundStyle(.white.opacity(0.68))
                        .lineLimit(1)
                }
            }

            Spacer(minLength: 8)

            if state.phase == .active {
                ProgressView()
                    .controlSize(.small)
            } else {
                Text("Done")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.7))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(Color.white.opacity(0.07), in: Capsule(style: .continuous))
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(Color.black.opacity(0.36), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(state.tint.opacity(0.30), lineWidth: 1)
        )
        .accessibilityIdentifier("transcript.tool-usage-banner")
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
                await createSessionWithCurrentWorkflowSettings()
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
                presentCompanionConnectAssistant()
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
        Button {
            showConnectionPopover.toggle()
        } label: {
            HStack(spacing: 6) {
                #if os(iOS)
                Image(systemName: companionConnectionIcon)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(companionConnectionColor)
                Text(companionConnectionState.label)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(companionConnectionColor)
                #else
                Circle()
                    .fill(connectionChipColor)
                    .frame(width: 8, height: 8)
                Text(store.selectedSessionRuntimeState.label)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(runtimeStateColor)
                #endif

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
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Session connection status")
        .accessibilityIdentifier("top.connection")
        .popover(isPresented: $showConnectionPopover, arrowEdge: .bottom) {
            ConnectionPopover(
                store: store,
                showConnectAssistant: $showCompanionConnectAssistant
            )
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
                                    if !pendingBottomScrollAfterThreadSwitch,
                                       store.requestOlderHistoryIfNeeded(for: session.id) {
                                        markPendingPrependAnchorForHistoryLoad()
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
                                            if !pendingBottomScrollAfterThreadSwitch,
                                               store.requestOlderHistoryIfNeeded(for: session.id) {
                                                markPendingPrependAnchorForHistoryLoad()
                                            }
                                        }
                                    }
                            }
                        }

                        Color.clear
                            .frame(height: 1)
                            .id(transcriptBottomAnchor)
                            .background(
                                GeometryReader { proxy in
                                    Color.clear.preference(
                                        key: TranscriptBottomOffsetPreferenceKey.self,
                                        value: proxy.frame(in: .named(transcriptScrollCoordinateSpaceName)).minY
                                    )
                                }
                            )
                    }
                    .padding(.horizontal, transcriptHorizontalPadding)
                    .padding(.vertical, 24)
                    .frame(maxWidth: 920)
                    .frame(maxWidth: .infinity)
                    .frame(minHeight: geometry.size.height, alignment: transcriptContentAlignment)
                }
                .id("transcript.session.\(session.id)")
                .defaultScrollAnchor(.bottom)
                .coordinateSpace(name: transcriptScrollCoordinateSpaceName)
                .simultaneousGesture(
                    DragGesture(minimumDistance: 4)
                        .onChanged { _ in
                            transcriptUserHasScrolled = true
                            pendingBottomPinPassesAfterThreadSwitch = 0
                        }
                )
                .onPreferenceChange(TranscriptBottomOffsetPreferenceKey.self) { bottomOffset in
                    transcriptIsNearBottom = bottomOffset <= geometry.size.height + 32
                }
                .onAppear {
                    scrollTranscriptToBottom(proxy: proxy, animated: false)
                }
                .onChange(of: store.selectedSessionID) { _, _ in
                    pendingPrependAnchorItemID = nil
                    pendingBottomScrollAfterThreadSwitch = true
                    pendingBottomPinPassesAfterThreadSwitch = 3
                }
                .onChange(of: transcriptTailIdentity) { _, _ in
                    if pendingPrependAnchorItemID != nil {
                        if transcriptIsNearBottom {
                            pendingPrependAnchorItemID = nil
                        } else {
                            return
                        }
                    }

                    if pendingBottomScrollAfterThreadSwitch {
                        pendingBottomScrollAfterThreadSwitch = false
                        pendingBottomPinPassesAfterThreadSwitch = max(pendingBottomPinPassesAfterThreadSwitch, 2)
                        scrollTranscriptToBottom(proxy: proxy, animated: false)
                        return
                    }

                    if pendingBottomPinPassesAfterThreadSwitch > 0 {
                        pendingBottomPinPassesAfterThreadSwitch -= 1
                        scrollTranscriptToBottom(proxy: proxy, animated: false)
                        return
                    }

                    if transcriptIsNearBottom || !transcriptUserHasScrolled {
                        scrollTranscriptToBottom(proxy: proxy, animated: true)
                    }
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
                    if transcriptIsNearBottom {
                        scrollTranscriptToBottom(proxy: proxy, animated: false)
                        return
                    }

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
        let shouldFitContentWidth = item.isPatchApplyEndEvent || item.isTokenCountEvent || item.isBackgroundEvent || item.isBrowserWorkflowEvent || item.isCollaborationProgressEvent
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

    private var transcriptTailIdentity: String {
        let sessionID = store.selectedSessionID?.uuidString ?? "none"
        guard let last = transcriptItems.last else {
            return "\(sessionID):empty:\(transcriptItems.count)"
        }

        return "\(sessionID):\(last.id):\(last.body.count):\(transcriptItems.count)"
    }

    private var selectedSessionTranscriptSourceKey: TranscriptSessionSourceKey? {
        guard let sessionID = store.selectedSessionID else {
            return nil
        }

        return transcriptSourceKey(for: store.selectedSessionItems, sessionID: sessionID)
    }

    private func scheduleTranscriptCacheRefresh(
        delayNanoseconds: UInt64,
        preserveBottomPin: Bool
    ) {
        transcriptRefreshTask?.cancel()
        transcriptRefreshTask = Task { @MainActor in
            if delayNanoseconds > 0 {
                try? await Task.sleep(nanoseconds: delayNanoseconds)
            }

            guard !Task.isCancelled else {
                return
            }

            refreshTranscriptCache()

            if !preserveBottomPin {
                pendingBottomPinPassesAfterThreadSwitch = max(
                    pendingBottomPinPassesAfterThreadSwitch,
                    pendingBottomScrollAfterThreadSwitch ? 3 : 0
                )
            }

            transcriptRefreshTask = nil
        }
    }

    private func refreshTranscriptCache() {
        guard let sessionID = store.selectedSessionID,
              let sourceKey = transcriptSourceKey(for: store.selectedSessionItems, sessionID: sessionID)
        else {
            cachedTranscriptItems = []
            taskActivityByStartItemID = [:]
            return
        }

        if let cached = transcriptCacheBySessionID[sessionID],
           cached.sourceKey == sourceKey {
            cachedTranscriptItems = cached.items
            taskActivityByStartItemID = cached.taskActivityByStartItemID
            touchTranscriptCache(sessionID)
            return
        }

        let visible = store.selectedSessionItems.filter {
            shouldIncludeInTranscript($0) && !$0.isReplayOmittedNotice
        }
        let replayPruned = pruneReplayHistoryCardsWhenRedundant(in: visible)
        let taskMerged = mergeTaskActivityIntoTaskCards(in: replayPruned)
        let reducedItems = dedupeAssistantMessagesWithinTurn(
            in: collapseConsecutiveReasoningCards(
                in: collapseLegacyExecLifecycleCards(
                    in: collapseExecOutputDeltaCards(
                        in: collapseStreamingDeltaCards(
                            in: dedupeToolCallLifecycleCards(
                                in: dedupeObsoleteBackgroundEvents(
                                    in: collapseTokenUsageBursts(
                                        in: removeRedundantPatchSummaries(
                                            from: dedupeObsoletePatchApplyCards(
                                                in: removeTrailingDiffCardsAfterAssistant(in: dedupeObsoleteDiffs(in: taskMerged.items))
                                            )
                                        )
                                    )
                                )
                            )
                        )
                    )
                )
            )
        )

        cachedTranscriptItems = reducedItems
        taskActivityByStartItemID = taskMerged.activityByStartItemID
        storeTranscriptCache(
            sessionID: sessionID,
            entry: TranscriptSessionCacheEntry(
                sourceKey: sourceKey,
                items: reducedItems,
                taskActivityByStartItemID: taskMerged.activityByStartItemID
            )
        )
    }

    private func dedupeToolCallLifecycleCards(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return items
        }

        let completedToolIdentities = Set(
            items.compactMap { item -> String? in
                guard let info = item.toolCallInfo,
                      info.phase == .completed
                else {
                    return nil
                }

                return toolCallIdentity(for: info)
            }
        )

        var result: [SessionStreamItem] = []
        result.reserveCapacity(items.count)
        var completedIndexByIdentity: [String: Int] = [:]

        for item in items {
            guard let info = item.toolCallInfo,
                  let identity = toolCallIdentity(for: info)
            else {
                result.append(item)
                continue
            }

            if info.phase == .started,
               completedToolIdentities.contains(identity) {
                continue
            }

            if info.phase == .completed,
               let existingIndex = completedIndexByIdentity[identity],
               let existingInfo = result[existingIndex].toolCallInfo,
               existingInfo.phase == .completed {
                if toolCallInfoQualityScore(info) > toolCallInfoQualityScore(existingInfo) {
                    result[existingIndex] = item
                }
                continue
            }

            if info.phase == .completed {
                completedIndexByIdentity[identity] = result.count
            }

            result.append(item)
        }

        return result
    }

    private func collapseExecOutputDeltaCards(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return items
        }

        let completedCallIDs = Set(
            items.compactMap { item -> String? in
                guard item.corePayloadType == "exec_command_end" else {
                    return nil
                }
                return item.execCommandCallID
            }
        )

        var result: [SessionStreamItem] = []
        result.reserveCapacity(items.count)
        var latestDeltaIndexByCallID: [String: Int] = [:]

        func removeDeltaCard(for callID: String) {
            guard let index = latestDeltaIndexByCallID.removeValue(forKey: callID),
                  result.indices.contains(index) else {
                return
            }

            result.remove(at: index)
            for (existingCallID, existingIndex) in latestDeltaIndexByCallID {
                if existingIndex > index {
                    latestDeltaIndexByCallID[existingCallID] = existingIndex - 1
                }
            }
        }

        for item in items {
            if let delta = item.execCommandOutputDeltaInfo {
                if completedCallIDs.contains(delta.callId) {
                    continue
                }

                if let existingIndex = latestDeltaIndexByCallID[delta.callId],
                   result.indices.contains(existingIndex) {
                    result[existingIndex] = item
                } else {
                    latestDeltaIndexByCallID[delta.callId] = result.count
                    result.append(item)
                }
                continue
            }

            if item.corePayloadType == "exec_command_end",
               let callID = item.execCommandCallID {
                removeDeltaCard(for: callID)
            }

            result.append(item)
        }

        return result
    }

    private func collapseLegacyExecLifecycleCards(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return items
        }

        var result: [SessionStreamItem] = []
        result.reserveCapacity(items.count)
        var pendingBeginIndexesBySignature: [String: [Int]] = [:]
        var pendingBeginIndexesInOrder: [Int] = []

        func shiftPendingIndexes(afterRemovedAt removedIndex: Int) {
            for (signature, indexes) in pendingBeginIndexesBySignature {
                let shifted = indexes.compactMap { index -> Int? in
                    if index == removedIndex {
                        return nil
                    }
                    return index > removedIndex ? index - 1 : index
                }
                if shifted.isEmpty {
                    pendingBeginIndexesBySignature.removeValue(forKey: signature)
                } else {
                    pendingBeginIndexesBySignature[signature] = shifted
                }
            }

            pendingBeginIndexesInOrder = pendingBeginIndexesInOrder.compactMap { index -> Int? in
                if index == removedIndex {
                    return nil
                }
                return index > removedIndex ? index - 1 : index
            }
        }

        for item in items {
            if item.corePayloadType == "exec_command_begin",
               item.execCommandCallID == nil {
                let beginIndex = result.count
                if let signature = execCommandSignatureWithoutCallID(for: item) {
                    pendingBeginIndexesBySignature[signature, default: []].append(beginIndex)
                }
                pendingBeginIndexesInOrder.append(beginIndex)
                result.append(item)
                continue
            }

            if item.corePayloadType == "exec_command_end",
               item.execCommandCallID == nil {
                var beginIndex: Int?

                if let signature = execCommandSignatureWithoutCallID(for: item),
                   var pendingIndexes = pendingBeginIndexesBySignature[signature],
                   let signatureIndex = pendingIndexes.popLast() {
                    beginIndex = signatureIndex
                    if pendingIndexes.isEmpty {
                        pendingBeginIndexesBySignature.removeValue(forKey: signature)
                    } else {
                        pendingBeginIndexesBySignature[signature] = pendingIndexes
                    }
                }

                if beginIndex == nil {
                    beginIndex = pendingBeginIndexesInOrder.popLast()
                }

                if let beginIndex {
                    pendingBeginIndexesInOrder.removeAll(where: { $0 == beginIndex })

                    if result.indices.contains(beginIndex) {
                        result.remove(at: beginIndex)
                        shiftPendingIndexes(afterRemovedAt: beginIndex)
                    }

                    result.append(item)
                    continue
                }
            }

            result.append(item)
        }

        return result
    }

    private func execCommandSignatureWithoutCallID(for item: SessionStreamItem) -> String? {
        guard let payloadObject = item.event?.payload?.objectValue,
              let commandValue = payloadObject["command"]
        else {
            return nil
        }

        switch commandValue {
        case .array(let values):
            let parts = values
                .compactMap { $0.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
            guard !parts.isEmpty else {
                return nil
            }
            return parts.joined(separator: " ")
        case .string(let command):
            let trimmed = command.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : trimmed
        default:
            return nil
        }
    }

    private func toolCallInfoQualityScore(_ info: ToolCallInfo) -> Int {
        var score = 0
        if !info.command.isEmpty {
            score += 4
        }
        let normalizedName = info.name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if normalizedName != "tool" && normalizedName != "." && !normalizedName.isEmpty {
            score += 3
        }
        if !info.arguments.isEmpty {
            score += 2
        }
        if !info.output.isEmpty {
            score += 1
        }
        if info.success != nil {
            score += 1
        }
        return score
    }

    private func toolCallIdentity(for info: ToolCallInfo) -> String? {
        if let callId = info.callId,
           !callId.isEmpty {
            return "call:\(callId)"
        }

        let normalizedName = info.name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if normalizedName == "shell" {
            // Legacy exec begin/end records can be missing call_id. Avoid
            // command-only identities here so repeated shell commands do not
            // collapse into one card across a turn.
            return nil
        }

        let normalizedCommand = info.command.trimmingCharacters(in: .whitespacesAndNewlines)
        if !normalizedCommand.isEmpty {
            return "cmd:\(info.name.lowercased())|\(normalizedCommand)"
        }

        guard !info.arguments.isEmpty else {
            return nil
        }

        let argumentSignature = info.arguments
            .map { "\($0.key)=\($0.value)" }
            .joined(separator: "|")
        if argumentSignature.isEmpty {
            return nil
        }

        return "args:\(info.name.lowercased())|\(argumentSignature)"
    }

    private func shouldIncludeInTranscript(_ item: SessionStreamItem) -> Bool {
        if !showActivityEvents {
            // "Activity off" should feel like chat-only mode: keep only real
            // conversation turns and hide tool/system chatter.
            if item.assistantMessageText != nil || item.isAssistantDeltaEvent {
                return true
            }

            if let userText = item.userMessageText,
               !userText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
               !item.isImagePlaceholderUserMessage {
                return true
            }

            return false
        }

        if !item.shouldHideFromTranscript {
            return true
        }

        return item.isOptionalActivityEvent
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
        dedupeAssistantMessagesWithinTurnItems(items)
    }

    private func collapseStreamingDeltaCards(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return items
        }

        var collapsed: [SessionStreamItem] = []
        collapsed.reserveCapacity(items.count)

        for item in items {
            if let assistantDelta = item.assistantDeltaText {
                if let last = collapsed.last,
                   last.isAssistantDeltaEvent,
                   shouldMergeStreamingDelta(last: last, incoming: item),
                   let payloadType = last.corePayloadType ?? item.corePayloadType,
                   let merged = mergedStreamingDeltaItem(
                       base: last,
                       incoming: item,
                       payloadType: payloadType,
                       mergedText: (last.assistantDeltaText ?? "") + assistantDelta
                   ) {
                    collapsed[collapsed.count - 1] = merged
                    continue
                }

                collapsed.append(item)
                continue
            }

            if let reasoningDelta = item.reasoningDeltaText {
                if let last = collapsed.last,
                   last.isReasoningDeltaEvent,
                   shouldMergeStreamingDelta(last: last, incoming: item),
                   let payloadType = last.corePayloadType ?? item.corePayloadType,
                   let merged = mergedStreamingDeltaItem(
                       base: last,
                       incoming: item,
                       payloadType: payloadType,
                       mergedText: (last.reasoningDeltaText ?? "") + reasoningDelta
                   ) {
                    collapsed[collapsed.count - 1] = merged
                    continue
                }

                collapsed.append(item)
                continue
            }

            collapsed.append(item)
        }

        return collapsed
    }

    private func shouldMergeStreamingDelta(last: SessionStreamItem, incoming: SessionStreamItem) -> Bool {
        guard last.type == "core_event", incoming.type == "core_event" else {
            return false
        }

        let lastID = last.event?.id.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let incomingID = incoming.event?.id.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""

        if !lastID.isEmpty, !incomingID.isEmpty {
            return lastID == incomingID
        }

        return true
    }

    private func mergedStreamingDeltaItem(
        base: SessionStreamItem,
        incoming: SessionStreamItem,
        payloadType: String,
        mergedText: String
    ) -> SessionStreamItem? {
        guard base.type == "core_event",
              let baseEvent = base.event
        else {
            return nil
        }

        let payload = JSONValue.object([
            "type": .string(payloadType),
            "delta": .string(mergedText),
        ])

        let mergedEvent = CoreEventPayload(
            id: baseEvent.id,
            eventSeq: incoming.event?.eventSeq ?? baseEvent.eventSeq,
            kind: incoming.event?.kind ?? baseEvent.kind,
            payload: payload
        )

        return SessionStreamItem(
            type: base.type,
            sessionId: base.sessionId,
            seq: base.seq,
            event: mergedEvent,
            rev: incoming.rev ?? base.rev,
            text: base.text,
            cursor: base.cursor,
            sourceClientId: base.sourceClientId,
            level: base.level,
            message: base.message
        )
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

    private func removeTrailingDiffCardsAfterAssistant(in items: [SessionStreamItem]) -> [SessionStreamItem] {
        func trimmedSegment(_ segment: [SessionStreamItem]) -> [SessionStreamItem] {
            guard !segment.isEmpty else {
                return segment
            }

            var trailingDiffStart = segment.count
            while trailingDiffStart > 0,
                  segment[trailingDiffStart - 1].turnDiffText != nil {
                trailingDiffStart -= 1
            }

            guard trailingDiffStart < segment.count else {
                return segment
            }

            let hasAssistantBeforeTrailingDiff = segment[..<trailingDiffStart]
                .contains(where: { $0.assistantMessageText != nil })
            if hasAssistantBeforeTrailingDiff {
                return Array(segment[..<trailingDiffStart])
            }

            return segment
        }

        var result: [SessionStreamItem] = []
        result.reserveCapacity(items.count)

        var segment: [SessionStreamItem] = []
        segment.reserveCapacity(64)

        for item in items {
            if item.isConversationBoundaryEvent {
                result.append(contentsOf: trimmedSegment(segment))
                segment.removeAll(keepingCapacity: true)
                result.append(item)
                continue
            }

            segment.append(item)
        }

        result.append(contentsOf: trimmedSegment(segment))
        return result
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
                    await createSessionWithCurrentWorkflowSettings()
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
                    IOSComposerTextView(
                        text: $composerDraft,
                        isFocused: composerIsFocused,
                        measuredHeight: $composerMeasuredHeight,
                        multilineBehavior: selectedMultilineBehavior,
                        onFocusChange: { composerIsFocused = $0 },
                        onSubmit: { submitComposerAction(text: $0) },
                        onEditorKeyCommand: handleComposerKeyCommand
                    )
                    .frame(height: composerEditorHeight)
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

    private var composerControlRows: AnyView {
        #if os(iOS)
        AnyView(
            VStack(spacing: 0) {
            HStack(spacing: 8) {
                if isCompactPhoneLayout {
                    Menu {
                        Section("Model") {
                            ForEach(modelOptions, id: \.self) { option in
                                Button(option) {
                                    selectedModel = option
                                    selectedReasoningLevel = normalizedReasoningSelection(current: selectedReasoningLevel)
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
                                selectedReasoningLevel = normalizedReasoningSelection(current: selectedReasoningLevel)
                            }
                        }
                    } label: {
                        Label(selectedModelDisplayLabel, systemImage: "chevron.down")
                    }
                    .menuStyle(.borderlessButton)

                    Menu {
                        ForEach(reasoningOptions, id: \.self) { option in
                            Button(option) {
                                selectedReasoningLevel = option
                            }
                        }
                    } label: {
                        Label("Reasoning: \(selectedReasoningDisplayLabel)", systemImage: "chevron.down")
                    }
                    .menuStyle(.borderlessButton)
                }

                if isCompactPhoneLayout {
                    Button {
                        slashLauncherOpenedByTyping = false
                        slashCommandQuery = ""
                        showSlashCommandLauncher = true
                    } label: {
                        Image(systemName: "slash")
                            .font(.caption.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.white.opacity(0.66))
                    .frame(width: 22, height: 22)
                    .background(Color.white.opacity(0.05), in: Circle())
                    .accessibilityIdentifier("composer.slash")
                    .accessibilityLabel("Slash commands")

                    Button {
                        guard ideContextEnabled else {
                            return
                        }
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
                    .disabled(!ideContextEnabled)

                    Spacer(minLength: 6)

                    Button {
                        toggleAutoSpeakAssistant()
                    } label: {
                        Image(systemName: voicePlaybackIconName)
                            .font(.caption.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(autoSpeakAssistant ? .green : .white.opacity(0.66))
                    .frame(width: 22, height: 22)
                    .background(Color.white.opacity(0.05), in: Circle())
                    .accessibilityIdentifier("composer.auto-speak-toggle")
                    .accessibilityLabel(voicePlaybackAccessibilityLabel)
                    .accessibilityHint(voicePlaybackAccessibilityHint)

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
                        slashLauncherOpenedByTyping = false
                        slashCommandQuery = ""
                        showSlashCommandLauncher = true
                    } label: {
                        Label("Slash", systemImage: "slash")
                    }
                    .buttonStyle(.plain)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.72))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color.white.opacity(0.08), in: Capsule(style: .continuous))
                    .overlay(
                        Capsule(style: .continuous)
                            .stroke(Color.white.opacity(0.12), lineWidth: 1)
                    )
                    .accessibilityIdentifier("composer.slash")
                    #if os(macOS)
                    .keyboardShortcut("k", modifiers: [.command])
                    #endif

                    Button {
                        guard ideContextEnabled else {
                            return
                        }
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
                    .background(Color.white.opacity(0.08), in: Capsule(style: .continuous))
                    .overlay(
                        Capsule(style: .continuous)
                            .stroke(Color.white.opacity(0.12), lineWidth: 1)
                    )
                    .accessibilityIdentifier("composer.context")
                    .disabled(!ideContextEnabled)
                    #if os(macOS)
                    .keyboardShortcut("p", modifiers: [.command, .shift])
                    #endif

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
                        toggleAutoSpeakAssistant()
                    } label: {
                        Image(systemName: voicePlaybackIconName)
                            .font(.caption.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(autoSpeakAssistant ? .green : .white.opacity(0.66))
                    .frame(width: 24, height: 24)
                    .background(Color.white.opacity(0.05), in: Circle())
                    .accessibilityIdentifier("composer.auto-speak-toggle")
                    .accessibilityLabel(voicePlaybackAccessibilityLabel)
                    .accessibilityHint(voicePlaybackAccessibilityHint)

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
        )
        #else
        AnyView(
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
                            selectedReasoningLevel = normalizedReasoningSelection(current: selectedReasoningLevel)
                        }
                    }
                } label: {
                    Label(selectedModelDisplayLabel, systemImage: "chevron.down")
                }
                .menuStyle(.borderlessButton)

                Menu {
                    ForEach(reasoningOptions, id: \.self) { option in
                        Button(option) {
                            selectedReasoningLevel = option
                        }
                    }
                } label: {
                    Label(selectedReasoningDisplayLabel, systemImage: "chevron.down")
                }
                .menuStyle(.borderlessButton)

                Button {
                    slashLauncherOpenedByTyping = false
                    slashCommandQuery = ""
                    showSlashCommandLauncher = true
                } label: {
                    Label("Slash", systemImage: "slash")
                }
                .buttonStyle(.plain)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.white.opacity(0.78))
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.white.opacity(0.08), in: Capsule(style: .continuous))
                .overlay(
                    Capsule(style: .continuous)
                        .stroke(Color.white.opacity(0.12), lineWidth: 1)
                )
                .accessibilityIdentifier("composer.slash")
                #if os(macOS)
                .keyboardShortcut("k", modifiers: [.command])
                #endif

                Button {
                    guard ideContextEnabled else {
                        return
                    }
                    ensureContextIndexLoaded()
                    showContextPicker = true
                } label: {
                    Label("Context", systemImage: "at")
                }
                .buttonStyle(.plain)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.white.opacity(0.78))
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.white.opacity(0.08), in: Capsule(style: .continuous))
                .overlay(
                    Capsule(style: .continuous)
                        .stroke(Color.white.opacity(0.12), lineWidth: 1)
                )
                .accessibilityIdentifier("composer.context")
                .disabled(!ideContextEnabled)
                #if os(macOS)
                .keyboardShortcut("p", modifiers: [.command, .shift])
                #endif

                Button {
                    showActivityEvents.toggle()
                } label: {
                    Label(showActivityEvents ? "Activity on" : "Activity off", systemImage: showActivityEvents ? "waveform.path.ecg" : "waveform.path.ecg.rectangle")
                }
                .buttonStyle(.borderless)

                Spacer()

                Button {
                    toggleAutoSpeakAssistant()
                } label: {
                    Image(systemName: voicePlaybackIconName)
                        .font(.caption.weight(.semibold))
                }
                .buttonStyle(.plain)
                .foregroundStyle(autoSpeakAssistant ? .green : .secondary)
                .frame(width: 24, height: 24)
                .background(Color.white.opacity(0.05), in: Circle())
                .accessibilityIdentifier("composer.auto-speak-toggle")
                .accessibilityLabel(voicePlaybackAccessibilityLabel)
                .accessibilityHint(voicePlaybackAccessibilityHint)

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
        )
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
        #if os(iOS)
        return "Connect to your Mac companion"
        #else
        return "Connect to your local session"
        #endif
    }

    private var emptyStateSubtitle: String {
        if store.connectionState == .connected {
            return "Start a new thread to mirror Codex events in real time."
        }
        #if os(iOS)
        return "Scan a pairing QR or import a pairing code from your Mac companion."
        #else
        return "Use a local loopback endpoint, then pick a thread to begin."
        #endif
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
                await createSessionWithCurrentWorkflowSettings()
            } else {
                #if os(iOS)
                presentCompanionConnectAssistant()
                #else
                await store.connect()
                #endif
            }
        }
    }

    private var canSubmit: Bool {
        canSendTurns && !composerDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var filteredSlashCommands: [ComposerSlashCommand] {
        ComposerSlashCommandCatalog.filteredCoreSet(query: slashCommandQuery)
    }

    private func slashCommandQueryFromComposerDraft(_ draft: String) -> String? {
        guard !showContextPicker
        else {
            return nil
        }

        let line = draft
            .replacingOccurrences(of: "\r\n", with: "\n")
            .components(separatedBy: "\n")
            .last ?? draft
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.hasPrefix("/") else {
            return nil
        }

        let remainder = String(trimmed.dropFirst())
        if remainder.isEmpty {
            return ""
        }

        if remainder.contains("/") {
            return nil
        }

        let token = remainder
            .split(whereSeparator: { $0.isWhitespace })
            .first
            .map(String.init) ?? ""

        if token.range(of: "^[A-Za-z0-9_-]*$", options: .regularExpression) == nil {
            return nil
        }

        return token.lowercased()
    }

    private func syncSlashCommandLauncher(with draft: String) {
        guard let query = slashCommandQueryFromComposerDraft(draft) else {
            if slashLauncherOpenedByTyping,
               showSlashCommandLauncher {
                showSlashCommandLauncher = false
            }
            return
        }

        slashCommandQuery = query
        if !showSlashCommandLauncher {
            slashLauncherOpenedByTyping = true
            showSlashCommandLauncher = true
        }
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
        guard ideContextEnabled,
              trailingMentionMatch != nil
        else {
            return []
        }

        return filteredContextPaths(query: inlineContextQuery)
    }

    private var displayedInlineContextPaths: [String] {
        Array(filteredInlineContextPaths.prefix(6))
    }

    private var filteredContextPickerPaths: [String] {
        guard ideContextEnabled else {
            return []
        }

        return filteredContextPaths(query: contextPickerQuery)
    }

    private var showsInlineContextSuggestions: Bool {
        ideContextEnabled
            && trailingMentionMatch != nil
            && !showContextPicker
            && !displayedInlineContextPaths.isEmpty
    }

    private struct ComposerActivityBadge {
        let label: String
        let icon: String
        let tint: Color
    }

    private enum ToolUsageBannerPhase {
        case active
        case collapsed
    }

    private struct ToolUsageBannerState {
        let phase: ToolUsageBannerPhase
        let title: String
        let detail: String?
        let icon: String
        let tint: Color
    }

    private struct TranscriptBottomOffsetPreferenceKey: PreferenceKey {
        static let defaultValue: CGFloat = .greatestFiniteMagnitude

        static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
            value = nextValue()
        }
    }

    private struct TranscriptSessionSourceKey: Equatable {
        let sessionID: UUID
        let itemCount: Int
        let firstSeq: UInt64
        let middleSeq: UInt64
        let lastSeq: UInt64
        let lastRev: UInt64?
    }

    private struct TranscriptSessionCacheEntry {
        let sourceKey: TranscriptSessionSourceKey
        let items: [SessionStreamItem]
        let taskActivityByStartItemID: [String: [String]]
    }

    private var toolUsageBannerState: ToolUsageBannerState? {
        guard store.selectedSessionID != nil else {
            return nil
        }

        for item in store.selectedSessionItems.reversed() {
            guard item.type == "core_event",
                  let payloadType = item.event?.payload?.typeHint
            else {
                continue
            }

            // Treat a user turn boundary as the cutoff for pinned tool activity.
            if payloadType == "user_message" || payloadType == "turn_aborted" {
                return nil
            }

            if let state = toolUsageBannerState(for: item, payloadType: payloadType) {
                return state
            }
        }

        return nil
    }

    private func toolUsageBannerState(for item: SessionStreamItem, payloadType: String) -> ToolUsageBannerState? {
        if let browserWorkflow = item.browserWorkflowEvent {
            let phase: ToolUsageBannerPhase = browserWorkflow.status == .inProgress ? .active : .collapsed
            let tint: Color = {
                switch browserWorkflow.status {
                case .inProgress:
                    return Color.blue.opacity(0.9)
                case .succeeded:
                    return Color.green.opacity(0.9)
                case .failed:
                    return Color.red.opacity(0.9)
                }
            }()

            return ToolUsageBannerState(
                phase: phase,
                title: browserWorkflow.headline,
                detail: condensedToolUsageDetail(browserWorkflow.detailLines.first),
                icon: "globe",
                tint: tint
            )
        }

        if let collaborationProgress = item.collaborationProgressEvent {
            let phase: ToolUsageBannerPhase = collaborationProgress.status == .inProgress ? .active : .collapsed
            let tint: Color = {
                switch collaborationProgress.status {
                case .inProgress:
                    return Color.indigo.opacity(0.9)
                case .succeeded:
                    return Color.green.opacity(0.9)
                case .failed:
                    return Color.red.opacity(0.9)
                }
            }()

            return ToolUsageBannerState(
                phase: phase,
                title: collaborationProgress.headline,
                detail: condensedToolUsageDetail(collaborationProgress.detailLines.first),
                icon: "person.2.wave.2",
                tint: tint
            )
        }

        if payloadType == "exec_command_end",
           let exec = item.execCommandInfo {
            let succeeded = (exec.exitCode ?? 0) == 0
            var detailParts: [String] = []
            if !exec.command.isEmpty {
                detailParts.append(exec.command)
            }
            if let duration = exec.duration,
               !duration.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                detailParts.append(duration)
            }

            return ToolUsageBannerState(
                phase: .collapsed,
                title: succeeded ? "Command finished" : "Command failed",
                detail: detailParts.isEmpty ? nil : detailParts.joined(separator: " • "),
                icon: succeeded ? "checkmark.circle.fill" : "exclamationmark.triangle.fill",
                tint: succeeded ? Color.green.opacity(0.9) : Color.red.opacity(0.9)
            )
        }

        if payloadType == "exec_command_begin" {
            return ToolUsageBannerState(
                phase: .active,
                title: "Running command",
                detail: condensedToolUsageDetail(item.body),
                icon: "terminal",
                tint: Color.green.opacity(0.9)
            )
        }

        if payloadType == "exec_command_output_delta" {
            return ToolUsageBannerState(
                phase: .active,
                title: "Streaming command output",
                detail: condensedToolUsageDetail(item.body),
                icon: "terminal",
                tint: Color.green.opacity(0.9)
            )
        }

        if payloadType == "patch_apply_begin" {
            return ToolUsageBannerState(
                phase: .active,
                title: "Applying patch",
                detail: condensedToolUsageDetail(item.body),
                icon: "doc.badge.gearshape",
                tint: Color.cyan.opacity(0.9)
            )
        }

        if payloadType == "patch_apply_end" {
            return ToolUsageBannerState(
                phase: .collapsed,
                title: "Patch applied",
                detail: condensedToolUsageDetail(item.body),
                icon: "checkmark.circle.fill",
                tint: Color.green.opacity(0.9)
            )
        }

        if payloadType == "tool_call_begin"
            || payloadType == "mcp_tool_call_begin"
            || payloadType == "custom_tool_call_begin"
            || payloadType == "custom_tool_call_update"
        {
            let detail = {
                guard let info = item.toolCallInfo else {
                    return condensedToolUsageDetail(item.body)
                }
                if !info.command.isEmpty {
                    return condensedToolUsageDetail(info.command)
                }
                if let firstArg = info.arguments.first {
                    return condensedToolUsageDetail("\(firstArg.key): \(firstArg.value)")
                }
                return condensedToolUsageDetail(item.body)
            }()

            return ToolUsageBannerState(
                phase: .active,
                title: "Running tool",
                detail: detail,
                icon: "wrench.and.screwdriver",
                tint: Color.cyan.opacity(0.9)
            )
        }

        if payloadType == "tool_call_end"
            || payloadType == "mcp_tool_call_end"
            || payloadType == "custom_tool_call_end"
        {
            let info = item.toolCallInfo
            let failed: Bool = {
                if let success = info?.success {
                    return !success
                }
                let normalized = item.body.lowercased()
                return normalized.contains("failed") || normalized.contains("error")
            }()
            let detail = {
                if let info {
                    if !info.command.isEmpty {
                        return condensedToolUsageDetail(info.command)
                    }
                    if !info.output.isEmpty {
                        return condensedToolUsageDetail(info.output)
                    }
                }
                return condensedToolUsageDetail(item.body)
            }()

            return ToolUsageBannerState(
                phase: .collapsed,
                title: failed ? "Tool failed" : "Tool finished",
                detail: detail,
                icon: failed ? "exclamationmark.triangle.fill" : "checkmark.circle.fill",
                tint: failed ? Color.red.opacity(0.9) : Color.green.opacity(0.9)
            )
        }

        return nil
    }

    private func condensedToolUsageDetail(_ value: String?) -> String? {
        guard let value else {
            return nil
        }

        let normalized = value
            .replacingOccurrences(of: "\r\n", with: "\n")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty else {
            return nil
        }

        let firstLine = normalized
            .components(separatedBy: "\n")
            .first?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? normalized

        guard !firstLine.isEmpty else {
            return nil
        }

        if firstLine.count <= 160 {
            return firstLine
        }

        let end = firstLine.index(firstLine.startIndex, offsetBy: 160)
        return "\(firstLine[..<end])…"
    }

    private var composerActivityBadge: ComposerActivityBadge? {
        for item in store.selectedSessionItems.reversed() {
            guard item.type == "core_event",
                  let payloadType = item.event?.payload?.typeHint
            else {
                continue
            }

            if payloadType == "agent_message"
                || payloadType == "user_message"
                || payloadType == "turn_aborted"
                || payloadType == "task_complete"
                || payloadType == "exec_command_end"
                || payloadType == "patch_apply_end"
                || payloadType == "turn_diff"
            {
                return nil
            }

            if payloadType == "agent_reasoning" || payloadType == "agent_reasoning_section_break" || payloadType == "task_started" {
                return ComposerActivityBadge(label: "Thinking…", icon: "brain.head.profile", tint: Color.yellow.opacity(0.92))
            }

            if payloadType == "exec_command_begin" || payloadType == "exec_command_output_delta" {
                return ComposerActivityBadge(label: "Executing…", icon: "terminal", tint: Color.green.opacity(0.9))
            }

            if payloadType == "tool_call_begin" {
                return ComposerActivityBadge(label: "Using tools…", icon: "wrench.and.screwdriver", tint: Color.cyan.opacity(0.9))
            }

            if payloadType == "web_search_begin" {
                return ComposerActivityBadge(label: "Browsing…", icon: "globe", tint: Color.blue.opacity(0.9))
            }

            if payloadType == "mcp_tool_call_begin",
               item.isBrowserWorkflowEvent {
                return ComposerActivityBadge(label: "Browsing…", icon: "globe", tint: Color.blue.opacity(0.9))
            }

            if payloadType == "web_search_end" || payloadType == "web_search_complete" {
                return nil
            }

            if payloadType == "mcp_tool_call_end",
               item.isBrowserWorkflowEvent {
                return nil
            }

            if payloadType.hasPrefix("collab_") {
                if payloadType.hasSuffix("_begin") {
                    return ComposerActivityBadge(label: "Coordinating…", icon: "person.2.wave.2", tint: Color.indigo.opacity(0.9))
                }

                if payloadType.hasSuffix("_end") {
                    return nil
                }
            }

            if payloadType == "patch_apply_begin" {
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
        guard ideContextEnabled else {
            indexedContextRootPath = nil
            indexedContextFilePaths = []
            contextIndexLoading = false
            return
        }

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
        guard ideContextEnabled else {
            return
        }

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
        slashLauncherOpenedByTyping = false
        slashCommandQuery = ""

        switch command.actionID {
        case .insertCommand(let commandText):
            appendComposerCommand(commandText)
        case .newThread:
            Task {
                await createSessionWithCurrentWorkflowSettings()
            }
        case .refreshThreads:
            Task {
                await store.refreshSessions()
            }
        case .openSettings:
            presentSettings(.general)
        case .openContextPicker:
            guard ideContextEnabled else {
                return
            }
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

    private func presentCompanionConnectAssistant() {
        #if os(iOS)
        showCompanionConnectAssistant = true
        #else
        Task {
            await store.connect()
        }
        #endif
    }

    private func composerPrimaryAction() {
        Task {
            if store.connectionState != .connected {
                #if os(iOS)
                presentCompanionConnectAssistant()
                #else
                await store.connect()
                #if os(macOS)
                focusComposerEditor(forceActivateApp: true)
                #endif
                #endif
                return
            }

            if store.selectedSession == nil {
                await createSessionWithCurrentWorkflowSettings()
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
            await store.submitComposer(
                text: text,
                model: selectedModelIdentifier,
                reasoningEffort: selectedReasoningEffort
            )
        }
    }

    private func createSessionWithCurrentWorkflowSettings() async {
        await store.createSession(
            cwd: nil,
            model: selectedModelIdentifier,
            reasoningEffort: selectedReasoningEffort
        )
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
        let minHeight: CGFloat = isCompactPhoneLayout ? 36 : 48
        let maxHeight: CGFloat = isCompactPhoneLayout ? 140 : 176
        return min(maxHeight, max(minHeight, composerMeasuredHeight))
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

        if item.isPatchApplyEndEvent || item.isTokenCountEvent || item.isBackgroundEvent || item.isBrowserWorkflowEvent || item.isCollaborationProgressEvent {
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

        if item.isBrowserWorkflowEvent {
            return 760 + widthDelta
        }

        if item.isCollaborationProgressEvent {
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

        if item.isBrowserWorkflowEvent {
            return 520
        }

        if item.isCollaborationProgressEvent {
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

    private var selectedModelDisplayLabel: String {
        normalizedModelSelection(current: selectedModel)
    }

    private var selectedReasoningDisplayLabel: String {
        normalizedReasoningSelection(current: selectedReasoningLevel)
    }

    private var selectedModelIdentifier: String {
        let normalizedLabel = normalizedModelSelection(current: selectedModel)
        if let option = modelPickerOptions.first(where: {
            $0.label.caseInsensitiveCompare(normalizedLabel) == .orderedSame
        }) {
            return option.model
        }

        if let canonical = WorkflowSettings.canonicalModelIdentifier(from: normalizedLabel) {
            return canonical
        }

        return WorkflowSettings.defaultModelIdentifier
    }

    private var selectedReasoningEffort: String {
        let normalizedLabel = normalizedReasoningSelection(current: selectedReasoningLevel)
        if let matched = reasoningPickerOptions.first(where: {
            $0.label.caseInsensitiveCompare(normalizedLabel) == .orderedSame
                || $0.effort.caseInsensitiveCompare(normalizedLabel) == .orderedSame
        }) {
            return matched.effort
        }

        if let canonical = WorkflowSettings.canonicalReasoningEffort(from: normalizedLabel) {
            return canonical
        }

        return selectedModelPickerOption?.defaultReasoningEffort ?? WorkflowSettings.defaultReasoningEffort
    }

    private func availableModelPickerOptions() -> [ModelPickerOption] {
        if !store.availableModelOptions.isEmpty {
            var options: [ModelPickerOption] = []
            options.reserveCapacity(store.availableModelOptions.count)
            var seenModels: Set<String> = []

            for option in store.availableModelOptions {
                let model = option.model.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !model.isEmpty else {
                    continue
                }

                let modelKey = model.lowercased()
                if seenModels.contains(modelKey) {
                    continue
                }
                seenModels.insert(modelKey)

                let defaultReasoningEffort = WorkflowSettings.canonicalReasoningEffort(
                    from: option.defaultReasoningEffort
                ) ?? WorkflowSettings.defaultReasoningEffort
                let reasoningEfforts = option.supportedReasoningEfforts
                    .compactMap { WorkflowSettings.canonicalReasoningEffort(from: $0.reasoningEffort) }

                let displayName = option.displayName.trimmingCharacters(in: .whitespacesAndNewlines)
                let label = displayName.isEmpty ? formatModelName(model) : displayName

                options.append(
                    ModelPickerOption(
                        model: model,
                        label: label,
                        defaultReasoningEffort: defaultReasoningEffort,
                        supportedReasoningEfforts: reasoningEfforts
                    )
                )
            }

            if !options.isEmpty {
                return options
            }
        }

        var fallbackOptions: [ModelPickerOption] = []
        var seenModels: Set<String> = []

        func appendFallbackOption(model: String, label: String) {
            let trimmedModel = model.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmedModel.isEmpty else {
                return
            }

            let modelKey = trimmedModel.lowercased()
            guard seenModels.insert(modelKey).inserted else {
                return
            }

            let trimmedLabel = label.trimmingCharacters(in: .whitespacesAndNewlines)
            let normalizedLabel = trimmedLabel.isEmpty ? formatModelName(trimmedModel) : trimmedLabel

            fallbackOptions.append(
                ModelPickerOption(
                    model: trimmedModel,
                    label: normalizedLabel,
                    defaultReasoningEffort: WorkflowSettings.defaultReasoningEffort,
                    supportedReasoningEfforts: WorkflowSettings.fallbackReasoningEfforts
                )
            )
        }

        let selectedValue = selectedModel.trimmingCharacters(in: .whitespacesAndNewlines)
        if !selectedValue.isEmpty,
           let selectedIdentifier = WorkflowSettings.canonicalModelIdentifier(from: selectedValue) {
            appendFallbackOption(model: selectedIdentifier, label: selectedValue)
        }

        appendFallbackOption(
            model: WorkflowSettings.defaultModelIdentifier,
            label: WorkflowSettings.defaultModelLabel
        )

        return fallbackOptions
    }

    private func availableReasoningPickerOptions(
        for selectedModelOption: ModelPickerOption?
    ) -> [ReasoningPickerOption] {
        let efforts = selectedModelOption?.supportedReasoningEfforts.isEmpty == false
            ? (selectedModelOption?.supportedReasoningEfforts ?? WorkflowSettings.fallbackReasoningEfforts)
            : WorkflowSettings.fallbackReasoningEfforts

        var options: [ReasoningPickerOption] = []
        options.reserveCapacity(efforts.count)
        var seenEfforts: Set<String> = []

        for effort in efforts {
            let normalizedEffort = WorkflowSettings.canonicalReasoningEffort(from: effort) ?? effort
            let effortKey = normalizedEffort.lowercased()
            if seenEfforts.contains(effortKey) {
                continue
            }
            seenEfforts.insert(effortKey)

            options.append(
                ReasoningPickerOption(
                    effort: normalizedEffort,
                    label: WorkflowSettings.displayReasoningEffort(normalizedEffort)
                )
            )
        }

        return options
    }

    private func normalizedModelSelection(current: String) -> String {
        if let matched = modelPickerOptions.first(where: {
            $0.label.caseInsensitiveCompare(current) == .orderedSame
                || $0.model.caseInsensitiveCompare(current) == .orderedSame
        }) {
            return matched.label
        }

        if let preferred = modelPickerOptions.first(where: {
            $0.model.caseInsensitiveCompare(WorkflowSettings.defaultModelIdentifier) == .orderedSame
        }) {
            return preferred.label
        }

        return modelOptions.first ?? WorkflowSettings.defaultModelLabel
    }

    private func normalizedReasoningSelection(current: String) -> String {
        if let matched = reasoningPickerOptions.first(where: {
            $0.label.caseInsensitiveCompare(current) == .orderedSame
                || $0.effort.caseInsensitiveCompare(current) == .orderedSame
        }) {
            return matched.label
        }

        if let defaultEffort = selectedModelPickerOption?.defaultReasoningEffort,
           let matchedDefault = reasoningPickerOptions.first(where: {
               $0.effort.caseInsensitiveCompare(defaultEffort) == .orderedSame
           }) {
            return matchedDefault.label
        }

        return reasoningOptions.first ?? WorkflowSettings.defaultReasoningLabel
    }

    private func normalizeWorkflowSettings() {
        selectedModel = normalizedModelSelection(current: selectedModel)
        selectedReasoningLevel = normalizedReasoningSelection(current: selectedReasoningLevel)
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

    private func enforceIDEContextEnabled() {
        if !ideContextEnabled {
            ideContextEnabled = true
        }
    }

    private func scrollTranscriptToBottom(proxy: ScrollViewProxy, animated: Bool) {
        let action = {
            proxy.scrollTo(transcriptBottomAnchor, anchor: .bottom)
        }

        let followUpAction = {
            DispatchQueue.main.async {
                action()
            }
        }

        if animated {
            withAnimation(.easeOut(duration: 0.2)) {
                action()
            }
            followUpAction()
        } else {
            action()
            followUpAction()
        }
    }

    private func markPendingPrependAnchorForHistoryLoad() {
        if pendingBottomScrollAfterThreadSwitch || transcriptIsNearBottom || !transcriptUserHasScrolled {
            pendingPrependAnchorItemID = nil
            return
        }

        pendingPrependAnchorItemID = transcriptItems.first?.id
    }

    private func transcriptSourceKey(
        for items: [SessionStreamItem],
        sessionID: UUID
    ) -> TranscriptSessionSourceKey? {
        guard let first = items.first,
              let last = items.last
        else {
            return nil
        }

        let middle = items[items.count / 2]
        return TranscriptSessionSourceKey(
            sessionID: sessionID,
            itemCount: items.count,
            firstSeq: first.seq,
            middleSeq: middle.seq,
            lastSeq: last.seq,
            lastRev: last.rev
        )
    }

    private func touchTranscriptCache(_ sessionID: UUID) {
        transcriptCacheUsageOrder.removeAll(where: { $0 == sessionID })
        transcriptCacheUsageOrder.append(sessionID)
    }

    private func storeTranscriptCache(sessionID: UUID, entry: TranscriptSessionCacheEntry) {
        transcriptCacheBySessionID[sessionID] = entry
        touchTranscriptCache(sessionID)

        if transcriptCacheUsageOrder.count > transcriptSessionCacheLimit,
           let evictedSessionID = transcriptCacheUsageOrder.first {
            transcriptCacheUsageOrder.removeFirst()
            transcriptCacheBySessionID.removeValue(forKey: evictedSessionID)
        }
    }

    private func clearTranscriptSessionCaches() {
        transcriptCacheBySessionID.removeAll(keepingCapacity: true)
        transcriptCacheUsageOrder.removeAll(keepingCapacity: true)
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
                await store.submitComposer(
                    text: finalText,
                    model: selectedModelIdentifier,
                    reasoningEffort: selectedReasoningEffort
                )
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
        voiceOutput.speak(
            text,
            voiceIdentifier: selectedVoiceIdentifier,
            rate: Float(voicePlaybackRate)
        )
    }

    private func previewVoiceSample() {
        voiceOutput.stop()
        voiceOutput.speak(
            "Voice preview. Assistant responses will play with this voice and speed.",
            voiceIdentifier: selectedVoiceIdentifier,
            rate: Float(voicePlaybackRate)
        )
    }

    private func toggleAutoSpeakAssistant() {
        autoSpeakAssistant.toggle()
        if !autoSpeakAssistant {
            voiceOutput.stop()
        }
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

private struct PairingQRCodeView: View {
    let payload: String
    let sideLength: CGFloat

    var body: some View {
        if let qrImage {
            qrImage
                .resizable()
                .interpolation(.none)
                .scaledToFit()
                .frame(width: sideLength, height: sideLength)
                .background(Color.white, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .stroke(Color.secondary.opacity(0.3), lineWidth: 1)
                )
        } else {
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(Color.secondary.opacity(0.12))
                .frame(width: sideLength, height: sideLength)
                .overlay {
                    Text("QR unavailable")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
        }
    }

    private var qrImage: Image? {
        let context = CIContext()
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(payload.utf8)
        filter.correctionLevel = "M"

        guard let outputImage = filter.outputImage else {
            return nil
        }

        let scaled = outputImage.transformed(by: CGAffineTransform(scaleX: 8, y: 8))
        guard let cgImage = context.createCGImage(scaled, from: scaled.extent) else {
            return nil
        }

        #if os(macOS)
        let size = NSSize(width: cgImage.width, height: cgImage.height)
        let nsImage = NSImage(cgImage: cgImage, size: size)
        return Image(nsImage: nsImage)
        #else
        let uiImage = UIImage(cgImage: cgImage)
        return Image(uiImage: uiImage)
        #endif
    }
}

private struct ConnectionPopover: View {
    @ObservedObject var store: SessionMirrorStore
    @Binding var showConnectAssistant: Bool
    @State private var pairingCodeInput = ""

    private var companionTokenBinding: Binding<String> {
        Binding(
            get: { store.companionSessionToken ?? "" },
            set: { value in
                let trimmedValue = value.trimmingCharacters(in: .whitespacesAndNewlines)
                store.companionSessionToken = trimmedValue.isEmpty ? nil : trimmedValue
            }
        )
    }

    private var hasPendingCompanionApproval: Bool {
        for item in store.selectedSessionItems.reversed() {
            guard item.type == "core_event",
                  let payloadType = item.event?.payload?.typeHint
            else {
                continue
            }

            if payloadType == "exec_approval_request"
                || payloadType == "apply_patch_approval_request"
                || payloadType == "request_user_input"
            {
                return true
            }

            if payloadType == "agent_message"
                || payloadType == "user_message"
                || payloadType == "turn_aborted"
                || payloadType == "exec_command_begin"
                || payloadType == "patch_apply_begin"
                || payloadType == "user_input_answer"
            {
                return false
            }
        }

        return false
    }

    private var companionState: CompanionConnectionState {
        CompanionConnectionState.resolve(
            connectionState: store.connectionState,
            statusLine: store.statusLine,
            lastError: store.lastError,
            hasPendingApproval: hasPendingCompanionApproval
        )
    }

    private var companionStateColor: Color {
        switch companionState {
        case .discovering:
            return .blue
        case .pairRequired:
            return .orange
        case .approvalPending:
            return .yellow
        case .connected:
            return .green
        case .reconnecting:
            return .orange
        case .offline:
            return .red
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Session Connection")
                .font(.headline)

            VStack(alignment: .leading, spacing: 4) {
                Text(companionState.label)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(companionStateColor)
                Text(companionState.detail)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            #if os(iOS)
            Text("Every Code Companion on iOS connects to a Mac companion endpoint. No local backend runs on device.")
                .font(.caption2)
                .foregroundStyle(.secondary)
            #endif

            HStack(spacing: 8) {
                #if os(macOS)
                Button("Use localhost") {
                    store.endpoint = SessionMirrorStore.defaultEndpoint
                }
                .buttonStyle(.bordered)
                #endif

                if let lanEndpoint = store.companionLANEndpoint,
                   !lanEndpoint.isEmpty {
                    Button("Use LAN endpoint") {
                        store.endpoint = lanEndpoint
                    }
                    .buttonStyle(.bordered)
                }
            }

            TextField("ws://127.0.0.1:4317/ws", text: $store.endpoint)
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("connection.endpoint")

            TextField("Bearer token", text: companionTokenBinding)
                .textFieldStyle(.roundedBorder)
                .font(.caption.monospaced())
                .accessibilityIdentifier("connection.token")

            if let companionPairingCode = store.companionPairingCode {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Pairing code")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Text(companionPairingCode)
                        .font(.caption.monospaced())
                        .textSelection(.enabled)
                        .foregroundStyle(.primary)
                        .accessibilityIdentifier("connection.pairing-code")
                }

                PairingQRCodeView(payload: companionPairingCode, sideLength: 128)
                    .accessibilityIdentifier("connection.pairing-qr")
            }

            HStack(spacing: 8) {
                TextField("Paste pairing code", text: $pairingCodeInput)
                    .textFieldStyle(.roundedBorder)
                    .font(.caption.monospaced())
                    .accessibilityIdentifier("connection.pairing-import")

                Button("Import") {
                    let imported = store.importCompanionPairingCode(pairingCodeInput)
                    if imported {
                        pairingCodeInput = ""
                    }
                }
                .buttonStyle(.bordered)
                .disabled(pairingCodeInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .accessibilityIdentifier("connection.pairing-import-action")
            }

            if let lanEndpoint = store.companionLANEndpoint,
               !lanEndpoint.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    Text("LAN companion endpoint")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Text(lanEndpoint)
                        .font(.caption.monospaced())
                        .textSelection(.enabled)
                        .foregroundStyle(.primary)
                        .accessibilityIdentifier("connection.lan-endpoint")
                }
            }

            HStack(spacing: 8) {
                Button("Connect") {
                    #if os(iOS)
                    showConnectAssistant = true
                    #else
                    Task {
                        await store.connect()
                    }
                    #endif
                }
                #if os(macOS)
                .disabled(store.connectionState != .disconnected)
                #endif
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

#if os(iOS)
private struct CompanionConnectAssistantSheet: View {
    @ObservedObject var store: SessionMirrorStore

    @Environment(\.dismiss) private var dismiss

    @State private var selectedMethod: ConnectionMethod = .scanQR
    @State private var pairingCodeInput = ""
    @State private var manualEndpoint = ""
    @State private var manualToken = ""
    @State private var isApplying = false
    @State private var localError: String?

    private enum ConnectionMethod: String, CaseIterable, Identifiable {
        case scanQR = "Scan QR"
        case pasteCode = "Paste Code"
        case manual = "Manual"

        var id: String { rawValue }
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("Connect this iPhone/iPad to your Mac companion. The iOS app does not run a local backend.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Section {
                    Picker("Method", selection: $selectedMethod) {
                        ForEach(ConnectionMethod.allCases) { method in
                            Text(method.rawValue).tag(method)
                        }
                    }
                    .pickerStyle(.segmented)
                    .accessibilityIdentifier("connection.assistant.method")
                }

                switch selectedMethod {
                case .scanQR:
                    Section("Scan pairing QR") {
                        PairingCodeScannerPanel { payload in
                            Task {
                                await applyPairingCode(payload)
                            }
                        }

                        Button("Use Clipboard Pairing Code") {
                            let clipboardValue = UIPasteboard.general.string ?? ""
                            pairingCodeInput = clipboardValue
                            selectedMethod = .pasteCode
                        }
                        .disabled((UIPasteboard.general.string ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityIdentifier("connection.assistant.use-clipboard")
                    }
                case .pasteCode:
                    Section("Paste pairing code") {
                        TextField("ecccompanion://pair?...", text: $pairingCodeInput, axis: .vertical)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .font(.caption.monospaced())
                            .accessibilityIdentifier("connection.assistant.pairing-code")

                        Button(isApplying ? "Connecting…" : "Import and Connect") {
                            Task {
                                await applyPairingCode(pairingCodeInput)
                            }
                        }
                        .disabled(isApplying || pairingCodeInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityIdentifier("connection.assistant.import-connect")
                    }
                case .manual:
                    Section("Manual endpoint") {
                        TextField("wss://your-companion/ws", text: $manualEndpoint)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .font(.caption.monospaced())
                            .accessibilityIdentifier("connection.assistant.manual-endpoint")

                        TextField("Bearer token", text: $manualToken)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .font(.caption.monospaced())
                            .accessibilityIdentifier("connection.assistant.manual-token")

                        Button(isApplying ? "Connecting…" : "Save and Connect") {
                            Task {
                                await saveManualAndConnect()
                            }
                        }
                        .disabled(isApplying || manualEndpoint.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityIdentifier("connection.assistant.manual-connect")
                    }
                }

                if let localError,
                   !localError.isEmpty {
                    Section {
                        Text(localError)
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("Connect Companion")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Connect now") {
                        Task {
                            await reconnectWithCurrentConfiguration()
                        }
                    }
                    .disabled(isApplying)
                    .accessibilityIdentifier("connection.assistant.connect-now")
                }
            }
            .onAppear {
                manualEndpoint = store.endpoint
                manualToken = store.companionSessionToken ?? ""
            }
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
    }

    private func applyPairingCode(_ rawCode: String) async {
        let trimmedCode = rawCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedCode.isEmpty else {
            return
        }

        localError = nil
        isApplying = true
        defer {
            isApplying = false
        }

        let imported = store.importCompanionPairingCode(trimmedCode)
        guard imported else {
            localError = store.lastError ?? "Pairing code is invalid."
            return
        }

        pairingCodeInput = ""
        manualEndpoint = store.endpoint
        manualToken = store.companionSessionToken ?? ""
        await reconnectWithCurrentConfiguration()
    }

    private func saveManualAndConnect() async {
        let endpoint = manualEndpoint.trimmingCharacters(in: .whitespacesAndNewlines)
        let token = manualToken.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !endpoint.isEmpty else {
            localError = "Endpoint is required."
            return
        }

        localError = nil
        isApplying = true
        defer {
            isApplying = false
        }

        store.endpoint = endpoint
        store.companionSessionToken = token.isEmpty ? nil : token
        await reconnectWithCurrentConfiguration()
    }

    private func reconnectWithCurrentConfiguration() async {
        if store.connectionState != .disconnected {
            store.disconnect()
        }

        await store.connect()
        if store.connectionState == .connected {
            dismiss()
            return
        }

        if let error = store.lastError,
           !error.isEmpty {
            localError = error
        }
    }
}

private struct PairingCodeScannerPanel: View {
    let onScan: (String) -> Void

    var body: some View {
        if QRPairingScannerView.isAvailable {
            QRPairingScannerView(onScan: onScan)
                .frame(minHeight: 220)
                .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .stroke(Color.white.opacity(0.12), lineWidth: 1)
                )
                .accessibilityIdentifier("connection.assistant.scanner")
        } else {
            VStack(alignment: .leading, spacing: 8) {
                Text("QR scanning is unavailable on this device right now.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text("Use Paste Code or clipboard import instead.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, 8)
            .accessibilityIdentifier("connection.assistant.scanner-unavailable")
        }
    }
}

private struct QRPairingScannerView: UIViewControllerRepresentable {
    let onScan: (String) -> Void

    static var isAvailable: Bool {
        DataScannerViewController.isSupported && DataScannerViewController.isAvailable
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(onScan: onScan)
    }

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            recognizesMultipleItems: false,
            isHighFrameRateTrackingEnabled: false,
            isHighlightingEnabled: true
        )
        scanner.delegate = context.coordinator

        do {
            try scanner.startScanning()
        } catch {}

        return scanner
    }

    func updateUIViewController(_ uiViewController: DataScannerViewController, context: Context) {
        context.coordinator.onScan = onScan
    }

    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        var onScan: (String) -> Void
        private var didEmitCode = false

        init(onScan: @escaping (String) -> Void) {
            self.onScan = onScan
        }

        func dataScanner(
            _ dataScanner: DataScannerViewController,
            didAdd addedItems: [RecognizedItem],
            allItems: [RecognizedItem]
        ) {
            guard !didEmitCode else {
                return
            }

            for item in addedItems {
                guard case let .barcode(barcode) = item,
                      let payload = barcode.payloadStringValue?.trimmingCharacters(in: .whitespacesAndNewlines),
                      !payload.isEmpty
                else {
                    continue
                }

                didEmitCode = true
                dataScanner.stopScanning()
                onScan(payload)
                return
            }
        }

    }
}
#endif

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

#if os(iOS)
private struct IOSComposerTextView: UIViewRepresentable {
    @Binding var text: String
    var isFocused: Bool
    @Binding var measuredHeight: CGFloat
    var multilineBehavior: MultilineBehavior
    let onFocusChange: (Bool) -> Void
    let onSubmit: (String) -> Void
    let onEditorKeyCommand: (ComposerEditorKeyCommand) -> Bool

    func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    func makeUIView(context: Context) -> SubmitAwareTextView {
        let textView = SubmitAwareTextView()
        textView.backgroundColor = .clear
        textView.font = UIFont.preferredFont(forTextStyle: .body)
        textView.textColor = UIColor.white.withAlphaComponent(0.9)
        textView.tintColor = UIColor.white.withAlphaComponent(0.95)
        textView.adjustsFontForContentSizeCategory = true
        textView.autocorrectionType = .yes
        textView.autocapitalizationType = .sentences
        textView.smartQuotesType = .no
        textView.smartDashesType = .no
        textView.smartInsertDeleteType = .no
        textView.textContainerInset = UIEdgeInsets(top: 6, left: 0, bottom: 6, right: 0)
        textView.textContainer.lineFragmentPadding = 0
        textView.isScrollEnabled = true
        textView.keyboardDismissMode = .interactive
        textView.delegate = context.coordinator
        textView.onSubmit = {
            context.coordinator.commitPendingTextImmediately()
            onSubmit(textView.text)
        }
        textView.onEditorKeyCommand = onEditorKeyCommand
        textView.multilineBehavior = multilineBehavior
        textView.text = text

        DispatchQueue.main.async {
            measuredHeight = Self.measuredEditorHeight(for: textView)
        }

        return textView
    }

    func updateUIView(_ uiView: SubmitAwareTextView, context: Context) {
        context.coordinator.parent = self

        if uiView.text != text {
            context.coordinator.cancelPendingSync()
            uiView.text = text
        }

        uiView.onSubmit = {
            context.coordinator.commitPendingTextImmediately()
            onSubmit(uiView.text)
        }
        uiView.onEditorKeyCommand = onEditorKeyCommand
        uiView.multilineBehavior = multilineBehavior

        if isFocused,
           !uiView.isFirstResponder {
            uiView.becomeFirstResponder()
        } else if !isFocused,
                  uiView.isFirstResponder {
            uiView.resignFirstResponder()
        }

        let nextHeight = Self.measuredEditorHeight(for: uiView)
        if abs(nextHeight - measuredHeight) > 0.5 {
            measuredHeight = nextHeight
        }
    }

    static func measuredEditorHeight(for textView: UITextView) -> CGFloat {
        let fallbackWidth = UIScreen.main.bounds.width
        let width = max(1, textView.bounds.width > 0 ? textView.bounds.width : fallbackWidth)
        let fitting = textView.sizeThatFits(
            CGSize(width: width, height: CGFloat.greatestFiniteMagnitude)
        )
        return ceil(max(24, fitting.height))
    }

    final class Coordinator: NSObject, UITextViewDelegate {
        var parent: IOSComposerTextView

        init(parent: IOSComposerTextView) {
            self.parent = parent
        }

        func textViewDidChange(_ textView: UITextView) {
            let updated = textView.text ?? ""
            parent.measuredHeight = IOSComposerTextView.measuredEditorHeight(for: textView)

            if parent.text != updated {
                parent.text = updated
            }
        }

        func textViewDidBeginEditing(_ textView: UITextView) {
            _ = textView
            if !parent.isFocused {
                parent.onFocusChange(true)
            }
        }

        func textViewDidEndEditing(_ textView: UITextView) {
            _ = textView
            if parent.isFocused {
                parent.onFocusChange(false)
            }
        }

        func textView(
            _ textView: UITextView,
            shouldChangeTextIn range: NSRange,
            replacementText replacement: String
        ) -> Bool {
            guard replacement == "\n",
                  parent.multilineBehavior == .enter
            else {
                return true
            }

            let current = textView.text ?? ""
            let proposed = (current as NSString).replacingCharacters(in: range, with: "")
            if parent.text != proposed {
                parent.text = proposed
            }

            parent.onSubmit(proposed)
            return false
        }

        func cancelPendingSync() {
            // Intentionally empty: sync is immediate during textViewDidChange.
        }

        func commitPendingTextImmediately() {
            // Intentionally empty: sync is immediate during textViewDidChange.
        }
    }

    final class SubmitAwareTextView: UITextView {
        var onSubmit: (() -> Void)?
        var onEditorKeyCommand: ((ComposerEditorKeyCommand) -> Bool)?
        var multilineBehavior: MultilineBehavior = .enter

        override var keyCommands: [UIKeyCommand]? {
            [
                UIKeyCommand(input: "\r", modifierFlags: .command, action: #selector(handleCommandReturn)),
            ]
        }

        @objc private func handleCommandReturn() {
            onSubmit?()
        }
    }
}
#endif

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

        init(parent: MacComposerTextView) {
            self.parent = parent
        }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else {
                return
            }

            let updated = textView.string
            parent.measuredHeight = MacComposerTextView.measuredEditorHeight(for: textView)

            if parent.text != updated {
                parent.text = updated
            }
        }

        func cancelPendingSync() {
            // Intentionally empty: sync is immediate during textDidChange.
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

            if isLikelyShellCommandLine(trimmed) {
                flushParagraph()
                blocks.append(.code(language: "shell", text: trimmed))
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

    private static func isLikelyShellCommandLine(_ line: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              !trimmed.hasPrefix("`")
        else {
            return false
        }

        let normalizedLine = normalizedShellCandidate(trimmed)
        guard !normalizedLine.isEmpty else {
            return false
        }

        if normalizedLine.hasPrefix("#") ||
            normalizedLine.hasPrefix(">") ||
            normalizedLine.hasPrefix("-") ||
            normalizedLine.hasPrefix("*") {
            return false
        }

        if normalizedLine.hasSuffix(":") {
            return false
        }

        let tokens = normalizedLine.split(separator: " ")
        guard let firstToken = tokens.first,
              tokens.count >= 2
        else {
            return false
        }

        let commandToken = String(firstToken)
        guard commandToken.range(of: "^[A-Za-z0-9._~/-]+$", options: .regularExpression) != nil else {
            return false
        }

        let lowercasedLine = normalizedLine.lowercased()
        if lowercasedLine.hasPrefix("if ") ||
            lowercasedLine.hasPrefix("when ") ||
            lowercasedLine.hasPrefix("the ") ||
            lowercasedLine.hasPrefix("this ") ||
            lowercasedLine.hasPrefix("that ") {
            return false
        }

        let executable = commandToken
            .split(separator: "/")
            .last
            .map { String($0).lowercased() } ?? commandToken.lowercased()
        let knownCommands: Set<String> = [
            "bash", "bun", "cargo", "cat", "cd", "chmod", "code", "cp", "curl",
            "docker", "find", "gh", "git", "go", "jq", "ls", "make", "mv", "node",
            "npm", "open", "pnpm", "python", "python3", "rg", "rustup", "sed", "sh",
            "swift", "tar", "touch", "uv", "xcodebuild", "yarn", "zsh"
        ]

        let hasFlagToken = tokens.dropFirst().contains { token in
            token.hasPrefix("-") && token.count > 1
        }
        let hasPipeOrRedirect = normalizedLine.contains(" | ") ||
            normalizedLine.contains(" > ") ||
            normalizedLine.contains(" < ")
        let pathLikeCommand = commandToken.hasPrefix("./") ||
            commandToken.hasPrefix("/") ||
            commandToken.hasPrefix("~/")
        let commandLooksValid = knownCommands.contains(executable) || pathLikeCommand

        guard commandLooksValid else {
            return false
        }

        return hasFlagToken || hasPipeOrRedirect || pathLikeCommand
    }

    private static func normalizedShellCandidate(_ line: String) -> String {
        var candidate = line

        if let linkRegex = try? NSRegularExpression(pattern: #"\[([^\]]+)\]\(([^\)]+)\)"#) {
            let range = NSRange(candidate.startIndex..<candidate.endIndex, in: candidate)
            candidate = linkRegex.stringByReplacingMatches(
                in: candidate,
                options: [],
                range: range,
                withTemplate: "$1"
            )
        }

        candidate = candidate.replacingOccurrences(of: "`", with: "")
        return candidate.trimmingCharacters(in: .whitespacesAndNewlines)
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
    private let containsLinks: Bool

    init(text: String, baseColor: Color = Color.white.opacity(0.93), sessionID: UUID?) {
        let styled = Self.makeStyledAttributedString(from: text, baseColor: baseColor, sessionID: sessionID)
        self.attributed = styled
        self.containsLinks = styled.runs.contains(where: { $0.link != nil })
    }

    var body: some View {
        Group {
            if containsLinks {
                Text(attributed)
                    .textSelection(.disabled)
            } else {
                Text(attributed)
                    .textSelection(.enabled)
            }
        }
            .frame(maxWidth: .infinity, alignment: .leading)
            #if os(macOS)
            .onContinuousHover { phase in
                guard containsLinks else {
                    return
                }

                switch phase {
                case .active:
                    NSCursor.pointingHand.set()
                case .ended:
                    NSCursor.arrow.set()
                }
            }
            #endif
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
                attributed[range].foregroundColor = Color.cyan.opacity(0.95)
                attributed[range].underlineStyle = nil
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

        let knownFileNames = [
            "cargo.toml", "cargo.lock", "package.json", "package-lock.json",
            "pnpm-lock.yaml", "yarn.lock", "readme.md"
        ]

        if value.hasPrefix("~/") || value.hasPrefix("/") {
            return true
        }

        if value.contains("/") {
            let components = value
                .split(separator: "/", omittingEmptySubsequences: true)
                .map(String.init)
            guard components.count >= 2 else {
                return false
            }

            let hasDotComponent = components.contains { $0.contains(".") }
            let hasKnownFileName = components.contains { knownFileNames.contains($0.lowercased()) }
            let hasPathPrefix = value.hasPrefix("./") ||
                value.hasPrefix("../") ||
                value.hasPrefix("a/") ||
                value.hasPrefix("b/")

            if hasDotComponent || hasKnownFileName || hasPathPrefix || components.count >= 3 {
                return true
            }

            return false
        }

        let lowercase = value.lowercased()
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

        if line.hasPrefix("*** Begin Patch")
            || line.hasPrefix("*** End Patch")
            || line.hasPrefix("*** Update File: ")
            || line.hasPrefix("*** Add File: ")
            || line.hasPrefix("*** Delete File: ")
            || line.hasPrefix("*** Move to: ")
            || line.hasPrefix("*** End of File")
        {
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
            if let applyPatchPath = extractPathFromApplyPatchHeader(line) {
                return applyPatchPath
            }
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

    private static func extractPathFromApplyPatchHeader(_ line: String) -> String? {
        let prefixes = [
            "*** Update File: ",
            "*** Add File: ",
            "*** Delete File: ",
        ]

        for prefix in prefixes {
            guard line.hasPrefix(prefix) else {
                continue
            }

            let path = line.dropFirst(prefix.count).trimmingCharacters(in: .whitespacesAndNewlines)
            return path.isEmpty ? nil : path
        }

        return nil
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
        if text.hasPrefix("*** Update File: ")
            || text.hasPrefix("*** Add File: ")
            || text.hasPrefix("*** Delete File: ")
        {
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
        if text.hasPrefix("*** Begin Patch")
            || text.hasPrefix("*** End Patch")
            || text.hasPrefix("*** Move to: ")
            || text.hasPrefix("*** End of File")
        {
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

    private var collaborationProgress: CollaborationProgressEvent? {
        item.collaborationProgressEvent
    }

    private var browserWorkflow: BrowserWorkflowEvent? {
        item.browserWorkflowEvent
    }

    private var toolCallInfo: ToolCallInfo? {
        item.toolCallInfo
    }

    private var execOutputDeltaInfo: ExecCommandOutputDeltaInfo? {
        item.execCommandOutputDeltaInfo
    }

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

        if collaborationProgress != nil {
            return Color.indigo.opacity(0.10)
        }

        if browserWorkflow != nil {
            return Color.blue.opacity(0.08)
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

        if collaborationProgress != nil {
            return Color.indigo.opacity(0.30)
        }

        if browserWorkflow != nil {
            return Color.blue.opacity(0.26)
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

        if collaborationProgress != nil {
            return false
        }

        if browserWorkflow != nil {
            return false
        }

        if toolCallInfo != nil {
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
        item.isPatchApplyEndEvent || item.isTokenCountEvent || item.isBackgroundEvent || item.isTaskLifecycleEvent || collaborationProgress != nil || browserWorkflow != nil
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

        if collaborationProgress != nil {
            return false
        }

        if browserWorkflow != nil {
            return false
        }

        if toolCallInfo != nil {
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

        if collaborationProgress != nil {
            return true
        }

        if browserWorkflow != nil {
            return true
        }

        if toolCallInfo != nil {
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

        if collaborationProgress != nil {
            return 420
        }

        if browserWorkflow != nil {
            return 420
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
            return isActive ? 12 : 7
        }
        return isActive ? 18 : 10
    }

    private var diffExpandChunkSize: Int {
        usesCompactPhoneLayout ? 14 : 20
    }

    private var currentDiffLineLimit: Int {
        guard renderedAsDiff else {
            return 0
        }

        guard isActive else {
            return min(totalDiffLines, usesCompactPhoneLayout ? 8 : 11)
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
            } else if let collaborationProgress {
                collaborationProgressContent(collaborationProgress)
            } else if let browserWorkflow {
                browserWorkflowContent(browserWorkflow)
            } else if item.isTaskLifecycleEvent || item.isBackgroundEvent {
                taskActivityContent
            } else if let execDelta = execOutputDeltaInfo {
                execOutputDeltaContent(execDelta)
            } else if let toolCallInfo {
                toolCallContent(toolCallInfo)
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
                        .accessibilityIdentifier("approval.choice.\(choice.rawValue)")
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
                        .accessibilityIdentifier("approval.set-deny")
                        #if os(macOS)
                        .keyboardShortcut("d", modifiers: [.command])
                        #endif

                        Spacer()

                        Button("Send decision") {
                            onApproval(selectedDecision)
                        }
                        .buttonStyle(.borderedProminent)
                        .accessibilityIdentifier("approval.send-decision")
                        #if os(macOS)
                        .keyboardShortcut(.defaultAction)
                        #endif
                    }
                    .padding(.top, 4)
                }
                .accessibilityIdentifier("approval.card")
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

    private func collaborationStatusLabel(_ status: CollaborationProgressStatus) -> String {
        switch status {
        case .inProgress:
            return "In progress"
        case .succeeded:
            return "Complete"
        case .failed:
            return "Error"
        }
    }

    private func collaborationStatusIcon(_ status: CollaborationProgressStatus) -> String {
        switch status {
        case .inProgress:
            return "person.2.wave.2"
        case .succeeded:
            return "checkmark.circle.fill"
        case .failed:
            return "exclamationmark.triangle.fill"
        }
    }

    private func collaborationStatusColor(_ status: CollaborationProgressStatus) -> Color {
        switch status {
        case .inProgress:
            return Color.indigo.opacity(0.92)
        case .succeeded:
            return Color.green.opacity(0.9)
        case .failed:
            return Color.orange.opacity(0.95)
        }
    }

    @ViewBuilder
    private func collaborationProgressContent(_ progress: CollaborationProgressEvent) -> some View {
        let statusColor = collaborationStatusColor(progress.status)
        VStack(alignment: .leading, spacing: 9) {
            HStack(spacing: 8) {
                Image(systemName: collaborationStatusIcon(progress.status))
                    .font(.caption)
                    .foregroundStyle(statusColor)
                    .accessibilityHidden(true)

                Text(progress.headline)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.95))
                    .lineLimit(2)

                Spacer(minLength: 6)

                Text(collaborationStatusLabel(progress.status))
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(statusColor)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(statusColor.opacity(0.16), in: Capsule(style: .continuous))
                    .accessibilityIdentifier("collab.progress.status")
            }

            if !progress.detailLines.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(progress.detailLines.prefix(6).enumerated()), id: \.offset) { _, line in
                        HStack(alignment: .firstTextBaseline, spacing: 7) {
                            Text("•")
                                .font(.caption2)
                                .foregroundStyle(Color.white.opacity(0.54))
                            Text(line)
                                .font(.caption.monospaced())
                                .foregroundStyle(.white.opacity(0.84))
                                .lineSpacing(2)
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
                .accessibilityIdentifier("collab.progress.details")
            }

            if let artifactPreview = progress.artifactPreview,
               !artifactPreview.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Result")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)

                    Text(artifactPreview)
                        .font(.caption.monospaced())
                        .foregroundStyle(.white.opacity(0.83))
                        .lineSpacing(2)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(9)
                .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .stroke(Color.white.opacity(0.09), lineWidth: 1)
                )
                .accessibilityIdentifier("collab.progress.result")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Coordinator progress")
        .accessibilityHint(collaborationStatusLabel(progress.status))
    }

    private func browserWorkflowStatusLabel(_ status: BrowserWorkflowStatus) -> String {
        switch status {
        case .inProgress:
            return "In progress"
        case .succeeded:
            return "Complete"
        case .failed:
            return "Error"
        }
    }

    private func browserWorkflowStatusIcon(_ status: BrowserWorkflowStatus) -> String {
        switch status {
        case .inProgress:
            return "globe"
        case .succeeded:
            return "checkmark.circle.fill"
        case .failed:
            return "exclamationmark.triangle.fill"
        }
    }

    private func browserWorkflowStatusColor(_ status: BrowserWorkflowStatus) -> Color {
        switch status {
        case .inProgress:
            return Color.blue.opacity(0.9)
        case .succeeded:
            return Color.green.opacity(0.9)
        case .failed:
            return Color.orange.opacity(0.95)
        }
    }

    @ViewBuilder
    private func browserWorkflowContent(_ workflow: BrowserWorkflowEvent) -> some View {
        let statusColor = browserWorkflowStatusColor(workflow.status)
        VStack(alignment: .leading, spacing: 9) {
            HStack(spacing: 8) {
                Image(systemName: browserWorkflowStatusIcon(workflow.status))
                    .font(.caption)
                    .foregroundStyle(statusColor)
                    .accessibilityHidden(true)

                Text(workflow.headline)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.95))
                    .lineLimit(2)

                Spacer(minLength: 6)

                Text(browserWorkflowStatusLabel(workflow.status))
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(statusColor)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(statusColor.opacity(0.16), in: Capsule(style: .continuous))
                    .accessibilityIdentifier("browser.workflow.status")
            }

            if !workflow.detailLines.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(workflow.detailLines.prefix(5).enumerated()), id: \.offset) { _, line in
                        HStack(alignment: .firstTextBaseline, spacing: 7) {
                            Text("•")
                                .font(.caption2)
                                .foregroundStyle(Color.white.opacity(0.54))
                            Text(line)
                                .font(.caption.monospaced())
                                .foregroundStyle(.white.opacity(0.84))
                                .lineSpacing(2)
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
                .accessibilityIdentifier("browser.workflow.details")
            }

            if let artifactPreview = workflow.artifactPreview,
               !artifactPreview.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Artifact")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)

                    Text(artifactPreview)
                        .font(.caption.monospaced())
                        .foregroundStyle(.white.opacity(0.83))
                        .lineSpacing(2)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(9)
                .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .stroke(Color.white.opacity(0.09), lineWidth: 1)
                )
                .accessibilityIdentifier("browser.workflow.artifact")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Browser workflow")
        .accessibilityHint(browserWorkflowStatusLabel(workflow.status))
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

    private func toolCallStatusText(for info: ToolCallInfo) -> String {
        switch info.phase {
        case .started:
            return "Running"
        case .completed:
            if let success = info.success {
                return success ? "Success" : "Failed"
            }
            return "Completed"
        }
    }

    private func toolCallStatusColor(for info: ToolCallInfo) -> Color {
        switch info.phase {
        case .started:
            return Color.cyan.opacity(0.92)
        case .completed:
            if let success = info.success {
                return success ? Color.green.opacity(0.92) : Color.red.opacity(0.92)
            }
            return Color.white.opacity(0.75)
        }
    }

    private func displayToolName(_ rawValue: String) -> String {
        let trimmed = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return "Tool"
        }

        return trimmed
            .split(separator: " ")
            .map { token in
                guard let first = token.first else {
                    return String(token)
                }
                return "\(first.uppercased())\(token.dropFirst())"
            }
            .joined(separator: " ")
    }

    private func previewToolOutput(_ value: String) -> String {
        let normalized = normalizeToolOutputPreview(value)
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

    private func normalizeToolOutputPreview(_ value: String) -> String {
        let trimmed = normalizeExecOutput(value).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return ""
        }

        if let extracted = extractToolOutputText(from: trimmed, depth: 0) {
            return extracted
        }

        if let fallback = extractOutputFieldFallback(from: trimmed) {
            return fallback
        }

        return trimmed
    }

    private func extractOutputFieldFallback(from value: String) -> String? {
        guard let regex = try? NSRegularExpression(
            pattern: #""output"\s*:\s*"((?:\\.|[^"\\])*)""#,
            options: []
        ) else {
            return nil
        }

        let range = NSRange(value.startIndex..<value.endIndex, in: value)
        guard let match = regex.firstMatch(in: value, options: [], range: range),
              match.numberOfRanges > 1,
              let captureRange = Range(match.range(at: 1), in: value)
        else {
            return nil
        }

        let rawCapture = String(value[captureRange])
        let normalized = normalizeExecOutput(rawCapture)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return normalized.isEmpty ? nil : normalized
    }

    private func extractToolOutputText(from value: String, depth: Int) -> String? {
        guard depth < 5,
              let data = value.data(using: .utf8),
              let parsed = try? JSONSerialization.jsonObject(with: data)
        else {
            return nil
        }

        return extractToolOutputText(from: parsed, depth: depth + 1)
    }

    private func extractToolOutputText(from value: Any, depth: Int) -> String? {
        guard depth < 6 else {
            return nil
        }

        if let text = value as? String {
            let normalized = normalizeExecOutput(text).trimmingCharacters(in: .whitespacesAndNewlines)
            guard !normalized.isEmpty else {
                return nil
            }

            if let nested = extractToolOutputText(from: normalized, depth: depth + 1) {
                return nested
            }

            return normalized
        }

        if let array = value as? [Any] {
            let pieces = array.compactMap { extractToolOutputText(from: $0, depth: depth + 1) }
                .filter { !$0.isEmpty }
            guard !pieces.isEmpty else {
                return nil
            }
            return pieces.joined(separator: "\n")
        }

        guard let object = value as? [String: Any] else {
            return nil
        }

        let orderedKeys = ["output", "stdout", "stderr", "text", "message", "content", "result", "data", "Ok", "Err"]
        for key in orderedKeys {
            guard let candidate = object[key],
                  let extracted = extractToolOutputText(from: candidate, depth: depth + 1),
                  !extracted.isEmpty
            else {
                continue
            }
            return extracted
        }

        if let data = try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted]),
           let pretty = String(data: data, encoding: .utf8) {
            let normalized = normalizeExecOutput(pretty).trimmingCharacters(in: .whitespacesAndNewlines)
            if !normalized.isEmpty {
                return normalized
            }
        }

        return nil
    }

    private struct RenderedToolArgument: Hashable {
        let key: String
        let value: String
        let codeLanguage: String?
    }

    private func truncateToolField(_ value: String, maxCharacters: Int) -> String {
        if value.count <= maxCharacters {
            return value
        }

        let endIndex = value.index(value.startIndex, offsetBy: maxCharacters)
        return "\(value[..<endIndex])…"
    }

    private func renderedToolArguments(
        from value: JSONValue,
        fallbackKey: String
    ) -> [RenderedToolArgument] {
        switch value {
        case .object(let fields):
            return fields
                .keys
                .sorted()
                .filter { $0 != "command" }
                .compactMap { key in
                    guard let nested = fields[key] else {
                        return nil
                    }
                    return renderedToolArgument(key: key, value: nested)
                }
        case .null:
            return []
        default:
            return [renderedToolArgument(key: fallbackKey, value: value)]
        }
    }

    private func renderedToolArguments(for info: ToolCallInfo) -> [RenderedToolArgument] {
        guard let payloadObject = item.event?.payload?.objectValue else {
            return info.arguments.map { argument in
                RenderedToolArgument(key: argument.key, value: argument.value, codeLanguage: nil)
            }
        }

        if let invocationObject = payloadObject["invocation"]?.objectValue,
           let invocationArguments = invocationObject["arguments"] {
            return renderedToolArguments(from: invocationArguments, fallbackKey: "args")
        }

        if let parameters = payloadObject["parameters"] {
            return renderedToolArguments(from: parameters, fallbackKey: "params")
        }

        if let argumentsObject = payloadObject["arguments"]?.objectValue {
            return argumentsObject
                .keys
                .sorted()
                .filter { $0 != "command" }
                .compactMap { key in
                    guard let value = argumentsObject[key] else {
                        return nil
                    }
                    return renderedToolArgument(key: key, value: value)
                }
        }

        if let rawArguments = payloadObject["arguments_raw"]?.stringValue {
            let normalized = normalizeExecOutput(rawArguments).trimmingCharacters(in: .whitespacesAndNewlines)
            if normalized.isEmpty {
                return []
            }

            if let pretty = prettifiedJSONText(from: normalized) {
                return [RenderedToolArgument(key: "args", value: pretty, codeLanguage: "json")]
            }

            return [RenderedToolArgument(key: "args", value: truncateToolField(normalized, maxCharacters: 360), codeLanguage: nil)]
        }

        return info.arguments.map { argument in
            RenderedToolArgument(key: argument.key, value: argument.value, codeLanguage: nil)
        }
    }

    private func toolCallPatchDiffText(for info: ToolCallInfo) -> String? {
        let normalizedName = info.name
            .lowercased()
            .replacingOccurrences(of: " ", with: "_")

        let candidateTexts = toolCallPatchCandidateTexts(info)
        for candidate in candidateTexts {
            guard let patch = extractApplyPatchBlock(from: candidate) else {
                continue
            }

            if normalizedName == "apply_patch" || patch.contains("*** Update File:") || patch.contains("*** Add File:") || patch.contains("*** Delete File:") {
                return patch
            }
        }

        return nil
    }

    private func toolCallPatchCandidateTexts(_ info: ToolCallInfo) -> [String] {
        var candidates: [String] = []

        if !info.command.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            candidates.append(info.command)
        }

        if let payloadObject = item.event?.payload?.objectValue {
            if let invocationObject = payloadObject["invocation"]?.objectValue,
               let invocationArguments = invocationObject["arguments"] {
                switch invocationArguments {
                case .object(let fields):
                    if let input = fields["input"]?.stringValue {
                        candidates.append(input)
                    }
                    if let patch = fields["patch"]?.stringValue {
                        candidates.append(patch)
                    }
                    if let commandValue = fields["command"],
                       let commandText = flattenedToolCommandText(from: commandValue) {
                        candidates.append(commandText)
                    }
                default:
                    candidates.append(invocationArguments.pretty)
                }
            }

            if let parameters = payloadObject["parameters"] {
                switch parameters {
                case .object(let fields):
                    if let input = fields["input"]?.stringValue {
                        candidates.append(input)
                    }
                    if let patch = fields["patch"]?.stringValue {
                        candidates.append(patch)
                    }
                    if let commandValue = fields["command"],
                       let commandText = flattenedToolCommandText(from: commandValue) {
                        candidates.append(commandText)
                    }
                default:
                    candidates.append(parameters.pretty)
                }
            }

            if let arguments = payloadObject["arguments"]?.objectValue {
                if let input = arguments["input"]?.stringValue {
                    candidates.append(input)
                }
                if let patch = arguments["patch"]?.stringValue {
                    candidates.append(patch)
                }
                if let commandValue = arguments["command"],
                   let commandText = flattenedToolCommandText(from: commandValue) {
                    candidates.append(commandText)
                }
            }

            if let rawArguments = payloadObject["arguments_raw"]?.stringValue {
                candidates.append(rawArguments)
            }
        }

        return candidates
    }

    private func flattenedToolCommandText(from value: JSONValue) -> String? {
        switch value {
        case .string(let command):
            let trimmed = command.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : trimmed

        case .array(let values):
            let parts = values.compactMap(\.stringValue)
            guard !parts.isEmpty else {
                return nil
            }
            return parts.joined(separator: "\n")

        default:
            return nil
        }
    }

    private func extractApplyPatchBlock(from text: String) -> String? {
        let normalized = normalizeExecOutput(text)
            .replacingOccurrences(of: "\r\n", with: "\n")

        guard !normalized.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return nil
        }

        if let beginRange = normalized.range(of: "*** Begin Patch"),
           let endRange = normalized.range(of: "*** End Patch", range: beginRange.lowerBound..<normalized.endIndex) {
            let patch = String(normalized[beginRange.lowerBound..<endRange.upperBound])
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return patch.isEmpty ? nil : patch
        }

        let markers = ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
        if markers.contains(where: normalized.contains) {
            return normalized.trimmingCharacters(in: .whitespacesAndNewlines)
        }

        return nil
    }

    private func toolCommandPreview(_ command: String, forPatchDiff patchDiffText: String?) -> String {
        let trimmed = command.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return ""
        }

        guard patchDiffText != nil else {
            return trimmed
        }

        let normalized = normalizeExecOutput(trimmed)
        guard let beginRange = normalized.range(of: "*** Begin Patch"),
              let endRange = normalized.range(of: "*** End Patch", range: beginRange.lowerBound..<normalized.endIndex)
        else {
            return trimmed
        }

        var compact = normalized
        compact.replaceSubrange(beginRange.lowerBound..<endRange.upperBound, with: "[patch content shown below]")
        return compact.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func shouldHideToolArgument(_ argument: RenderedToolArgument, forPatchDiff patchDiffText: String?) -> Bool {
        guard patchDiffText != nil else {
            return false
        }

        let normalizedKey = argument.key.lowercased()
        if normalizedKey == "input" || normalizedKey == "patch" {
            return true
        }

        return argument.value.contains("*** Begin Patch")
    }

    private func renderedToolArgument(key: String, value: JSONValue) -> RenderedToolArgument {
        switch value {
        case .object, .array:
            return RenderedToolArgument(
                key: key,
                value: truncateToolField(value.pretty, maxCharacters: 700),
                codeLanguage: "json"
            )
        case .string(let raw):
            let normalized = normalizeExecOutput(raw).trimmingCharacters(in: .whitespacesAndNewlines)
            if let pretty = prettifiedJSONText(from: normalized) {
                return RenderedToolArgument(key: key, value: truncateToolField(pretty, maxCharacters: 700), codeLanguage: "json")
            }

            return RenderedToolArgument(
                key: key,
                value: truncateToolField(normalized, maxCharacters: 260),
                codeLanguage: normalized.contains("\n") ? "text" : nil
            )
        case .number(let number):
            return RenderedToolArgument(key: key, value: String(number), codeLanguage: nil)
        case .bool(let flag):
            return RenderedToolArgument(key: key, value: flag ? "true" : "false", codeLanguage: nil)
        case .null:
            return RenderedToolArgument(key: key, value: "null", codeLanguage: nil)
        }
    }

    private func prettifiedJSONText(from value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              let data = trimmed.data(using: .utf8),
              let parsed = try? JSONSerialization.jsonObject(with: data),
              let prettyData = try? JSONSerialization.data(withJSONObject: parsed, options: [.prettyPrinted]),
              let pretty = String(data: prettyData, encoding: .utf8)
        else {
            return nil
        }

        return pretty
    }

    @ViewBuilder
    private func execOutputDeltaContent(_ info: ExecCommandOutputDeltaInfo) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text("Tool output")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)

                Spacer(minLength: 0)

                Text(info.stream?.uppercased() ?? "STREAM")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(Color.cyan.opacity(0.9))

                Text("Streaming")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(Color.cyan.opacity(0.9))
            }

            AssistantCodeBlock(
                language: (info.stream ?? "output"),
                code: info.chunk.isEmpty ? "(waiting for output...)" : previewExecOutput(info.chunk)
            )
        }
    }

    @ViewBuilder
    private func toolCallContent(_ info: ToolCallInfo) -> some View {
        let statusColor = toolCallStatusColor(for: info)
        let patchDiffText = toolCallPatchDiffText(for: info)
        let arguments = renderedToolArguments(for: info)
        let visibleArguments = arguments.filter { argument in
            !shouldHideToolArgument(argument, forPatchDiff: patchDiffText)
        }
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text("Tool")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)

                Spacer(minLength: 0)

                if let callId = info.callId,
                   !callId.isEmpty {
                    Text(callId)
                        .font(.caption2.monospaced())
                        .foregroundStyle(.white.opacity(0.72))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 3)
                        .background(Color.white.opacity(0.06), in: Capsule(style: .continuous))
                }

                Text(toolCallStatusText(for: info))
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(statusColor)
            }

            Text(displayToolName(info.name))
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.white.opacity(0.93))

            let commandPreview = toolCommandPreview(info.command, forPatchDiff: patchDiffText)
            if !commandPreview.isEmpty {
                AssistantCodeBlock(language: "shell", code: commandPreview)
            }

            if let patchDiffText {
                let preview = BeautifulDiffPreview(
                    diffText: patchDiffText,
                    visibleLineLimit: isActive ? 22 : 12
                )

                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text("Patch preview")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.secondary)

                        Spacer(minLength: 0)

                        DiffSummaryPill(
                            fileText: preview.fileSummaryText,
                            extraFileCount: preview.extraFileCount,
                            additions: preview.additionsCount,
                            removals: preview.removalsCount
                        )
                    }

                    preview
                }
            }

            if !visibleArguments.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(Array(visibleArguments.prefix(6).enumerated()), id: \.offset) { _, argument in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(argument.key)
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.secondary)

                            if let codeLanguage = argument.codeLanguage {
                                AssistantCodeBlock(language: codeLanguage, code: argument.value)
                            } else {
                                Text(argument.value)
                                    .font(.caption.monospaced())
                                    .foregroundStyle(.white.opacity(0.84))
                                    .lineSpacing(2)
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                    }
                }
            }

            if info.phase == .completed {
                let outputPreview = previewToolOutput(info.output)
                VStack(alignment: .leading, spacing: 6) {
                    Text(outputPreview.isEmpty ? "Output · none" : "Output preview")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)

                    if outputPreview.isEmpty {
                        Text("No output captured")
                            .font(.caption.monospaced())
                            .foregroundStyle(.white.opacity(0.62))
                            .padding(.horizontal, 10)
                            .padding(.vertical, 7)
                            .background(Color.black.opacity(0.14), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    } else {
                        AssistantCodeBlock(language: "output", code: outputPreview)
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

func dedupeAssistantMessagesWithinTurnItems(_ items: [SessionStreamItem]) -> [SessionStreamItem] {
    guard !items.isEmpty else {
        return items
    }

    func assistantMessageSignature(for item: SessionStreamItem) -> String? {
        guard item.cardStyle == .assistant else {
            return nil
        }

        let normalized = item.body.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty else {
            return nil
        }

        return normalized
    }

    var seenAssistantBodies: Set<String> = []
    var sawCompletedAssistantMessageInTurn = false
    var keptReversed: [SessionStreamItem] = []
    keptReversed.reserveCapacity(items.count)

    for item in items.reversed() {
        if item.userMessageText != nil {
            seenAssistantBodies.removeAll(keepingCapacity: true)
            sawCompletedAssistantMessageInTurn = false
            keptReversed.append(item)
            continue
        }

        if item.cardStyle == .assistant,
           !item.isAssistantDeltaEvent {
            sawCompletedAssistantMessageInTurn = true
        }

        if item.isAssistantDeltaEvent,
           sawCompletedAssistantMessageInTurn {
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
    let companionPairingEntries: [CompanionPairingEntry]
    let canRotateCompanionToken: Bool
    let rotateCompanionToken: (() -> Void)?
    let createCompanionPairing: ((String?) -> Void)?
    let revokeCompanionPairing: ((String) -> Void)?
    let restoreCompanionPairing: ((String) -> Void)?
    let deleteCompanionPairing: ((String) -> Void)?
    let importedProfileState: ImportedProfileState?
    let importCLIProfile: ((URL) throws -> ImportedProfileState)?
    let clearImportedCLIProfile: (() -> Void)?
    @Binding var autoSpeakAssistant: Bool
    @Binding var preferredVoiceIdentifier: String
    @Binding var voicePlaybackRate: Double
    @Binding var autoSubmitVoice: Bool
    let previewVoiceSample: (() -> Void)?
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
    let modelOptions: [String]
    let reasoningOptions: [String]
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
    @State private var pairingCodeImportText = ""
    @State private var newPairingLabel = ""
    @State private var profileImportMessage: String?
    @State private var profileImportError: String?

    private static let pairingExpiryFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d, h:mm a"
        return formatter
    }()

    private static let profileImportFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d, yyyy h:mm a"
        return formatter
    }()

    private enum PairingStatus {
        case active
        case expired
        case revoked

        var label: String {
            switch self {
            case .active:
                "Active"
            case .expired:
                "Expired"
            case .revoked:
                "Revoked"
            }
        }

        var color: Color {
            switch self {
            case .active:
                .green
            case .expired:
                .orange
            case .revoked:
                .red
            }
        }
    }

    private struct VoiceSelectionItem: Identifiable {
        let identifier: String
        let label: String

        var id: String { identifier }
    }

    private var normalizedPreferredVoiceIdentifier: String {
        preferredVoiceIdentifier.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var voiceSelectionItems: [VoiceSelectionItem] {
        let availableItems = VoiceOutputController.availableVoiceOptions().map { option in
            VoiceSelectionItem(
                identifier: option.identifier,
                label: "\(option.displayName) (\(option.languageCode), \(option.qualityLabel))"
            )
        }

        var items: [VoiceSelectionItem] = [
            VoiceSelectionItem(
                identifier: "",
                label: "Automatic (Best available)"
            )
        ]
        items.append(contentsOf: availableItems)

        let selectedIdentifier = normalizedPreferredVoiceIdentifier
        if !selectedIdentifier.isEmpty,
           !items.contains(where: { $0.identifier == selectedIdentifier }) {
            items.append(
                VoiceSelectionItem(
                    identifier: selectedIdentifier,
                    label: "Saved voice (Unavailable on this device)"
                )
            )
        }

        return items
    }

    private var voicePlaybackRateLabel: String {
        String(format: "%.2f", voicePlaybackRate)
    }

    private let sandboxOptions = WorkflowSettings.sandboxOptions
    private let approvalPolicyOptions = WorkflowSettings.approvalPolicyOptions

    private var availableIDEs: [SessionIDESelection] {
        #if os(macOS)
        IDEAvailability.availableSelections()
        #else
        [.systemDefault]
        #endif
    }

    private var publicAppName: String {
        let info = Bundle.main.infoDictionary
        let displayName = info?["CFBundleDisplayName"] as? String
        if let displayName, displayName.isEmpty == false {
            return displayName
        }

        let bundleName = info?["CFBundleName"] as? String
        if let bundleName, bundleName.isEmpty == false {
            return bundleName
        }

        return "Every Code Companion"
    }

    private var publicAppAttributionText: String {
        "\(publicAppName) is an independent, unofficial client and is not affiliated with or endorsed by Every Code."
    }

    private var publicAppVersionText: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String
        let build = info?["CFBundleVersion"] as? String

        switch (version, build) {
        case let (version?, build?) where version.isEmpty == false && build.isEmpty == false:
            return "Version \(version) (\(build))"
        case let (version?, _) where version.isEmpty == false:
            return "Version \(version)"
        case let (_, build?) where build.isEmpty == false:
            return "Build \(build)"
        default:
            return "Version information unavailable"
        }
    }

    private var publicAppBuildMarkerText: String {
        if let info = Bundle.main.infoDictionary,
           let gitSHA = info["CODE_NATIVE_GIT_SHA"] as? String,
           !gitSHA.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return "Build commit: \(gitSHA)"
        }

        guard let executableURL = Bundle.main.executableURL,
              let attributes = try? FileManager.default.attributesOfItem(atPath: executableURL.path)
        else {
            return "Build marker unavailable"
        }

        let size = (attributes[.size] as? NSNumber)?.int64Value ?? 0
        let modifiedAt = attributes[.modificationDate] as? Date
        let marker = "\(String(size, radix: 16))"
        if let modifiedAt {
            let timestamp = Int(modifiedAt.timeIntervalSince1970)
            return "Build marker: \(marker)-\(String(timestamp, radix: 16))"
        }

        return "Build marker: \(marker)"
    }

    private var publicAppBinaryPathText: String {
        let bundlePath = Bundle.main.bundleURL.path
        return "Bundle path: \(bundlePath)"
    }

    private var isCompactSettingsLayout: Bool {
        #if os(iOS)
        return horizontalSizeClass == .compact
        #else
        return false
        #endif
    }

    private var pairingCodeEndpoint: String? {
        if let companionLANEndpoint = store.companionLANEndpoint {
            let trimmedLANEndpoint = companionLANEndpoint.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmedLANEndpoint.isEmpty {
                return trimmedLANEndpoint
            }
        }

        let trimmedEndpoint = store.endpoint.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedEndpoint.isEmpty {
            return nil
        }
        return trimmedEndpoint
    }

    private func pairingCode(for entry: CompanionPairingEntry) -> String? {
        guard let pairingCodeEndpoint else {
            return nil
        }

        return SessionMirrorStore.buildCompanionPairingCode(
            endpoint: pairingCodeEndpoint,
            token: entry.sessionToken,
            deviceID: entry.id,
            expiresAtUnixMs: entry.expiresAtUnixMs
        )
    }

    private func pairingStatus(for entry: CompanionPairingEntry) -> PairingStatus {
        let nowUnixMs = currentUnixTimeMilliseconds()
        if entry.isRevoked {
            return .revoked
        }
        if entry.isExpired(referenceUnixMs: nowUnixMs) {
            return .expired
        }
        return .active
    }

    private func pairingExpiryCaption(for entry: CompanionPairingEntry) -> String {
        let expiryDate = Date(timeIntervalSince1970: Double(entry.expiresAtUnixMs) / 1_000)
        return "Expires \(Self.pairingExpiryFormatter.string(from: expiryDate))"
    }

    private var importedProfileStatusText: String {
        guard let importedProfileState else {
            return "No imported CLI profile. Companion uses the app sandbox profile by default."
        }

        let importedAtText = Self.profileImportFormatter.string(from: importedProfileState.importedAt)
        return "Imported from \(importedProfileState.sourcePath) at \(importedAtText)."
    }

    private func importSelectedCLIProfile(from selectedDirectoryURL: URL) {
        guard let importCLIProfile else {
            profileImportError = "CLI profile import is unavailable."
            profileImportMessage = nil
            return
        }

        let accessedSecurityScope = selectedDirectoryURL.startAccessingSecurityScopedResource()
        defer {
            if accessedSecurityScope {
                selectedDirectoryURL.stopAccessingSecurityScopedResource()
            }
        }

        do {
            let importedState = try importCLIProfile(selectedDirectoryURL)
            let importedAtText = Self.profileImportFormatter.string(from: importedState.importedAt)
            profileImportMessage = "Imported profile from \(importedState.sourcePath) at \(importedAtText)."
            profileImportError = nil
        } catch {
            profileImportError = error.localizedDescription
            profileImportMessage = nil
        }
    }

    #if os(macOS)
    private func importCLIProfileFromPanel() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Import"
        panel.title = "Import CLI Profile"
        panel.message = "Select your home folder, .code, or .codex directory."
        NSApplication.shared.activate(ignoringOtherApps: true)
        let response = panel.runModal()
        guard response == .OK,
              let selectedDirectoryURL = panel.url
        else {
            self.profileImportMessage = nil
            self.profileImportError = "Import cancelled."
            return
        }

        importSelectedCLIProfile(from: selectedDirectoryURL)
    }

    private func importCLIProfileFromHomeDirectory() {
        let homeDirectoryURL = FileManager.default.homeDirectoryForCurrentUser
        importSelectedCLIProfile(from: homeDirectoryURL)
    }
    #endif

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
                .pickerStyle(.menu)
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

            SettingsRow(title: "IDE context", description: "IDE context hints are always enabled in the composer.") {
                Text("Always on")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
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

            SettingsInfoCard(text: publicAppAttributionText)
            SettingsInfoCard(text: publicAppVersionText)
            SettingsInfoCard(text: publicAppBuildMarkerText)
            SettingsInfoCard(text: publicAppBinaryPathText)

        case .configuration:
            SettingsRow(title: "Mirror endpoint", description: "WebSocket endpoint used by native clients.") {
                TextField("ws://127.0.0.1:4317/ws", text: $store.endpoint)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: isCompactSettingsLayout ? nil : 320)
            }

            SettingsRow(title: "Companion token", description: "Bearer token required by managed companion runtime.") {
                TextField(
                    "Bearer token",
                    text: Binding(
                        get: { store.companionSessionToken ?? "" },
                        set: { value in
                            let trimmedValue = value.trimmingCharacters(in: .whitespacesAndNewlines)
                            store.companionSessionToken = trimmedValue.isEmpty ? nil : trimmedValue
                        }
                    )
                )
                .textFieldStyle(.roundedBorder)
                .font(.caption.monospaced())
                .frame(width: isCompactSettingsLayout ? nil : 320)
            }

            SettingsRow(title: "Rotate token", description: "Reissue the companion token and restart runtime to revoke prior pairings.") {
                HStack(spacing: 8) {
                    Button("Rotate now") {
                        rotateCompanionToken?()
                    }
                    .buttonStyle(.bordered)
                    .disabled(canRotateCompanionToken == false || rotateCompanionToken == nil)

                    if canRotateCompanionToken == false {
                        Text("Managed by CODE_NATIVE_COMPANION_TOKEN")
                            .font(.caption2.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
            }

            SettingsRow(title: "Create pairing", description: "Mint a device-specific token and pairing code.") {
                HStack(spacing: 8) {
                    TextField("Device label (optional)", text: $newPairingLabel)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: isCompactSettingsLayout ? nil : 260)

                    Button("Create") {
                        createCompanionPairing?(newPairingLabel)
                        newPairingLabel = ""
                    }
                    .buttonStyle(.bordered)
                    .disabled(createCompanionPairing == nil)
                }
            }

            SettingsRow(title: "Paired devices", description: "Revoke or delete individual devices without rotating every token.") {
                if companionPairingEntries.isEmpty {
                    Text("No device-specific pairings yet.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                } else {
                    VStack(alignment: .leading, spacing: 12) {
                        ForEach(companionPairingEntries) { pairing in
                            VStack(alignment: .leading, spacing: 6) {
                                let status = pairingStatus(for: pairing)
                                HStack(spacing: 8) {
                                    Text(pairing.displayLabel)
                                        .font(.subheadline.weight(.semibold))

                                    Text(status.label)
                                        .font(.caption2.monospaced())
                                        .foregroundStyle(status.color)
                                }

                                Text(pairingExpiryCaption(for: pairing))
                                    .font(.caption2.monospaced())
                                    .foregroundStyle(.secondary)

                                let isActive = status == .active

                                HStack(spacing: 8) {
                                    if status == .revoked || status == .expired {
                                        Button("Restore") {
                                            restoreCompanionPairing?(pairing.id)
                                        }
                                        .buttonStyle(.bordered)
                                        .disabled(restoreCompanionPairing == nil)
                                    } else if isActive {
                                        Button("Revoke") {
                                            revokeCompanionPairing?(pairing.id)
                                        }
                                        .buttonStyle(.bordered)
                                        .disabled(revokeCompanionPairing == nil)
                                    }

                                    Button("Delete") {
                                        deleteCompanionPairing?(pairing.id)
                                    }
                                    .buttonStyle(.bordered)
                                    .disabled(deleteCompanionPairing == nil)
                                }

                                if isActive,
                                   let pairingCode = pairingCode(for: pairing) {
                                    HStack(alignment: .top, spacing: 10) {
                                        Text(pairingCode)
                                            .font(.caption.monospaced())
                                            .textSelection(.enabled)
                                            .frame(maxWidth: .infinity, alignment: .leading)

                                        PairingQRCodeView(payload: pairingCode, sideLength: 92)
                                    }
                                }
                            }
                            .padding(10)
                            .background(Color.secondary.opacity(0.06), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                    }
                }
            }

            SettingsRow(title: "Local pairing code", description: "Fallback pairing code tied to the local companion token.") {
                Group {
                    if let companionPairingCode = store.companionPairingCode {
                        HStack(alignment: .top, spacing: 12) {
                            Text(companionPairingCode)
                                .font(.caption.monospaced())
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)

                            PairingQRCodeView(payload: companionPairingCode, sideLength: 104)
                        }
                    } else {
                        Text("Pairing code unavailable until endpoint and token are set.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }

            SettingsRow(title: "Import pairing code", description: "Paste a pairing code to set endpoint and token together.") {
                HStack(spacing: 8) {
                    TextField("ecccompanion://pair?...", text: $pairingCodeImportText)
                        .textFieldStyle(.roundedBorder)
                        .font(.caption.monospaced())
                        .frame(width: isCompactSettingsLayout ? nil : 320)

                    Button("Import") {
                        let imported = store.importCompanionPairingCode(pairingCodeImportText)
                        if imported {
                            pairingCodeImportText = ""
                        }
                    }
                    .buttonStyle(.bordered)
                    .disabled(pairingCodeImportText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }

            SettingsRow(title: "CLI profile", description: "Import ~/.code and ~/.codex into the app sandbox so auth, config, and sessions match your local CLI.") {
                VStack(alignment: .leading, spacing: 8) {
                    Text(importedProfileStatusText)
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    #if os(macOS)
                    HStack(spacing: 8) {
                        Button("Import from folder…") {
                            profileImportMessage = "Opening folder picker..."
                            profileImportError = nil
                            importCLIProfileFromPanel()
                        }
                        .buttonStyle(.bordered)

                        Button("Import from home (~)") {
                            importCLIProfileFromHomeDirectory()
                        }
                        .buttonStyle(.bordered)

                        Button("Clear imported copy") {
                            clearImportedCLIProfile?()
                            profileImportMessage = "Cleared imported CLI profile."
                            profileImportError = nil
                        }
                        .buttonStyle(.bordered)
                        .disabled(clearImportedCLIProfile == nil || importedProfileState == nil)
                    }
                    #else
                    Text("Profile import is available on macOS.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                    #endif

                    if let profileImportMessage {
                        Text(profileImportMessage)
                            .font(.caption)
                            .foregroundStyle(.green)
                    }

                    if let profileImportError {
                        Text(profileImportError)
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            if let lanEndpoint = store.companionLANEndpoint,
               !lanEndpoint.isEmpty {
                SettingsRow(title: "LAN endpoint", description: "Share this endpoint with iOS/iPadOS clients on your local network.") {
                    Text(lanEndpoint)
                        .font(.caption.monospaced())
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
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

            SettingsRow(title: "Assistant voice", description: "Choose the voice used for spoken assistant responses.") {
                Picker(
                    "",
                    selection: Binding(
                        get: { normalizedPreferredVoiceIdentifier },
                        set: { preferredVoiceIdentifier = $0 }
                    )
                ) {
                    ForEach(voiceSelectionItems) { item in
                        Text(item.label).tag(item.identifier)
                    }
                }
                .pickerStyle(.menu)
                .frame(width: isCompactSettingsLayout ? nil : 320)
            }

            SettingsRow(title: "Voice speed", description: "Fine-tune spoken response speed.") {
                HStack(spacing: 10) {
                    Slider(
                        value: $voicePlaybackRate,
                        in: 0.35...0.58,
                        step: 0.01
                    )
                    .frame(width: isCompactSettingsLayout ? nil : 220)

                    Text(voicePlaybackRateLabel)
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                        .frame(width: 44, alignment: .trailing)
                }
            }

            SettingsRow(title: "Preview voice", description: "Play a short sample with current voice settings.") {
                Button("Play sample") {
                    previewVoiceSample?()
                }
                .buttonStyle(.bordered)
                .disabled(previewVoiceSample == nil)
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
                fallback: WorkflowSettings.defaultModelLabel
            )
            selectedReasoningLevel = WorkflowSettings.normalizedSelection(
                current: selectedReasoningLevel,
                options: reasoningOptions,
                fallback: WorkflowSettings.defaultReasoningLabel
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
