import AVFoundation
import Foundation

@MainActor
final class VoiceOutputController: NSObject, ObservableObject {
    struct VoiceOption: Identifiable, Hashable {
        let identifier: String
        let displayName: String
        let languageCode: String
        let qualityLabel: String
        let qualityRank: Int

        var id: String { identifier }
    }

    @Published private(set) var isSpeaking: Bool = false

    private let synthesizer = AVSpeechSynthesizer()
    private var cachedVoiceIdentifier: String?

    override init() {
        super.init()
        synthesizer.delegate = self
    }

    nonisolated static func availableVoiceOptions() -> [VoiceOption] {
        let voices = AVSpeechSynthesisVoice.speechVoices().map { voice in
            VoiceOption(
                identifier: voice.identifier,
                displayName: voice.name,
                languageCode: voice.language,
                qualityLabel: qualityLabel(for: voice.quality),
                qualityRank: qualityRank(for: voice.quality)
            )
        }

        return voices.sorted {
            if $0.qualityRank != $1.qualityRank {
                return $0.qualityRank > $1.qualityRank
            }

            if $0.displayName != $1.displayName {
                return $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending
            }

            return $0.languageCode.localizedCaseInsensitiveCompare($1.languageCode) == .orderedAscending
        }
    }

    func speak(_ text: String, voiceIdentifier: String? = nil, rate: Float = 0.46) {
        let normalized = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty else {
            return
        }

        if synthesizer.isSpeaking {
            synthesizer.stopSpeaking(at: .immediate)
        }

        let utterance = AVSpeechUtterance(string: normalized)
        utterance.voice = preferredVoice(identifier: voiceIdentifier)
        utterance.rate = min(max(rate, 0.35), 0.58)
        utterance.pitchMultiplier = 1.0

        synthesizer.speak(utterance)
    }

    func stop() {
        synthesizer.stopSpeaking(at: .immediate)
    }

    private func preferredVoice(identifier: String?) -> AVSpeechSynthesisVoice? {
        let trimmedIdentifier = identifier?.trimmingCharacters(in: .whitespacesAndNewlines)
        if let trimmedIdentifier,
           !trimmedIdentifier.isEmpty,
           let selectedVoice = AVSpeechSynthesisVoice(identifier: trimmedIdentifier) {
            return selectedVoice
        }

        if let cachedVoiceIdentifier,
           let cachedVoice = AVSpeechSynthesisVoice(identifier: cachedVoiceIdentifier) {
            return cachedVoice
        }

        let availableVoices = AVSpeechSynthesisVoice.speechVoices()
        let languageCandidates = preferredLanguageCandidates()

        for languageCode in languageCandidates {
            if let voice = bestVoice(in: availableVoices, languageCode: languageCode) {
                cachedVoiceIdentifier = voice.identifier
                return voice
            }
        }

        if let fallbackVoice = AVSpeechSynthesisVoice(language: "en-US") {
            cachedVoiceIdentifier = fallbackVoice.identifier
            return fallbackVoice
        }

        return nil
    }

    private func preferredLanguageCandidates() -> [String] {
        var candidates: [String] = []

        if let preferredLanguage = Locale.preferredLanguages.first,
           !preferredLanguage.isEmpty {
            candidates.append(preferredLanguage)
        }

        let currentLanguageCode = AVSpeechSynthesisVoice.currentLanguageCode()
        if !currentLanguageCode.isEmpty {
            candidates.append(currentLanguageCode)
        }

        candidates.append("en-US")
        candidates.append("en")

        var deduplicated: [String] = []
        var seen = Set<String>()
        for languageCode in candidates {
            let normalized = languageCode.lowercased()
            if seen.insert(normalized).inserted {
                deduplicated.append(languageCode)
            }
        }

        return deduplicated
    }

    private func bestVoice(in voices: [AVSpeechSynthesisVoice], languageCode: String) -> AVSpeechSynthesisVoice? {
        let normalized = languageCode.lowercased()
        let baseLanguage = normalized.split(separator: "-").first.map(String.init) ?? normalized

        let matchingVoices = voices.filter { voice in
            let voiceLanguage = voice.language.lowercased()
            if voiceLanguage == normalized {
                return true
            }

            if voiceLanguage == baseLanguage {
                return true
            }

            return voiceLanguage.hasPrefix("\(baseLanguage)-")
        }

        guard !matchingVoices.isEmpty else {
            return nil
        }

        return matchingVoices.max(by: { lhs, rhs in
            voicePriority(lhs, targetLanguageCode: normalized)
                < voicePriority(rhs, targetLanguageCode: normalized)
        })
    }

    private func voicePriority(_ voice: AVSpeechSynthesisVoice, targetLanguageCode: String) -> Int {
        var score = 0

        switch voice.quality {
        case .premium:
            score += 300
        case .enhanced:
            score += 200
        default:
            score += 100
        }

        let voiceLanguage = voice.language.lowercased()
        if voiceLanguage == targetLanguageCode {
            score += 80
        } else if voiceLanguage.hasPrefix("\(targetLanguageCode)-") {
            score += 40
        }

        if #available(macOS 14.0, iOS 17.0, tvOS 17.0, watchOS 10.0, *) {
            if voice.voiceTraits.contains(.isNoveltyVoice) {
                score -= 200
            }
        }

        if voice.name.localizedCaseInsensitiveContains("siri") {
            score += 20
        }

        return score
    }

    nonisolated private static func qualityRank(for quality: AVSpeechSynthesisVoiceQuality) -> Int {
        switch quality {
        case .premium:
            return 3
        case .enhanced:
            return 2
        case .default:
            return 1
        @unknown default:
            return 0
        }
    }

    nonisolated private static func qualityLabel(for quality: AVSpeechSynthesisVoiceQuality) -> String {
        switch quality {
        case .premium:
            return "Premium"
        case .enhanced:
            return "Enhanced"
        case .default:
            return "Default"
        @unknown default:
            return "Other"
        }
    }
}

extension VoiceOutputController: AVSpeechSynthesizerDelegate {
    nonisolated func speechSynthesizer(
        _ synthesizer: AVSpeechSynthesizer,
        didStart utterance: AVSpeechUtterance
    ) {
        Task { @MainActor in
            self.isSpeaking = true
        }
    }

    nonisolated func speechSynthesizer(
        _ synthesizer: AVSpeechSynthesizer,
        didFinish utterance: AVSpeechUtterance
    ) {
        Task { @MainActor in
            self.isSpeaking = false
        }
    }

    nonisolated func speechSynthesizer(
        _ synthesizer: AVSpeechSynthesizer,
        didCancel utterance: AVSpeechUtterance
    ) {
        Task { @MainActor in
            self.isSpeaking = false
        }
    }
}
