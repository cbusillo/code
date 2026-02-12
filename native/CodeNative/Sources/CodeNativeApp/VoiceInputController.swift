import AVFoundation
import Foundation
import Speech

@MainActor
final class VoiceInputController: NSObject, ObservableObject {
    @Published private(set) var isRecording: Bool = false
    @Published private(set) var liveTranscript: String = ""
    @Published private(set) var lastError: String?
    @Published private(set) var isSpeechAuthorized: Bool = false
    @Published private(set) var isMicAuthorized: Bool = false

    var requiresOnDeviceRecognition: Bool = true

    private let audioEngine = AVAudioEngine()
    private let speechRecognizer = SFSpeechRecognizer(locale: Locale(identifier: "en-US"))
    private var recognitionRequest: SFSpeechAudioBufferRecognitionRequest?
    private var recognitionTask: SFSpeechRecognitionTask?

    func requestPermissions() async -> Bool {
        let speechAllowed = await requestSpeechPermission()
        let micAllowed = await requestMicrophonePermission()

        isSpeechAuthorized = speechAllowed
        isMicAuthorized = micAllowed

        if !speechAllowed {
            lastError = "Speech recognition permission not granted."
        } else if !micAllowed {
            lastError = "Microphone permission not granted."
        } else {
            lastError = nil
        }

        return speechAllowed && micAllowed
    }

    func startRecording(onUpdate: @escaping (_ text: String, _ isFinal: Bool) -> Void) async {
        guard !isRecording else {
            return
        }

        guard await requestPermissions() else {
            return
        }

        guard let speechRecognizer else {
            lastError = "Speech recognizer is unavailable for this locale."
            return
        }

        recognitionTask?.cancel()
        recognitionTask = nil

        let recognitionRequest = SFSpeechAudioBufferRecognitionRequest()
        recognitionRequest.shouldReportPartialResults = true
        if requiresOnDeviceRecognition {
            recognitionRequest.requiresOnDeviceRecognition = speechRecognizer.supportsOnDeviceRecognition
        }
        self.recognitionRequest = recognitionRequest

        let inputNode = audioEngine.inputNode
        inputNode.removeTap(onBus: 0)

        let format = inputNode.outputFormat(forBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            self?.recognitionRequest?.append(buffer)
        }

        do {
            audioEngine.prepare()
            try audioEngine.start()
        } catch {
            lastError = "Failed to start microphone: \(error.localizedDescription)"
            inputNode.removeTap(onBus: 0)
            return
        }

        liveTranscript = ""
        lastError = nil
        isRecording = true

        recognitionTask = speechRecognizer.recognitionTask(with: recognitionRequest) { [weak self] result, error in
            guard let self else {
                return
            }

            if let result {
                Task { @MainActor in
                    self.liveTranscript = result.bestTranscription.formattedString
                    onUpdate(result.bestTranscription.formattedString, result.isFinal)
                }
            }

            if let error {
                Task { @MainActor in
                    self.lastError = "Speech recognition error: \(error.localizedDescription)"
                    self.stopRecording()
                }
            }
        }
    }

    @discardableResult
    func stopRecording() -> String {
        if isRecording {
            audioEngine.stop()
            audioEngine.inputNode.removeTap(onBus: 0)
            recognitionRequest?.endAudio()
        }

        recognitionTask?.cancel()
        recognitionTask = nil
        recognitionRequest = nil
        isRecording = false

        return liveTranscript
    }

    private func requestSpeechPermission() async -> Bool {
        await withCheckedContinuation { continuation in
            SFSpeechRecognizer.requestAuthorization { status in
                continuation.resume(returning: status == .authorized)
            }
        }
    }

    private func requestMicrophonePermission() async -> Bool {
        await withCheckedContinuation { continuation in
            AVCaptureDevice.requestAccess(for: .audio) { granted in
                continuation.resume(returning: granted)
            }
        }
    }
}
