import AVFoundation
import Foundation
@preconcurrency import Speech

enum SpeechError: Error, Sendable {
    case notAuthorized
    case unavailable
    case audioEngineFailure(String)
    case recognizerFailure(String)
}

actor SpeechService {
    private let recognizer: SFSpeechRecognizer?
    private let audioEngine = AVAudioEngine()
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?

    init(locale: Locale = .current) {
        self.recognizer = SFSpeechRecognizer(locale: locale)
    }

    static func requestAuthorization() async -> Bool {
        let speechOk = await withCheckedContinuation { cont in
            SFSpeechRecognizer.requestAuthorization { status in
                cont.resume(returning: status == .authorized)
            }
        }
        let micOk = await AVAudioApplication.requestRecordPermission()
        return speechOk && micOk
    }

    /// Stream transcription updates from live microphone input. The stream
    /// yields the best-hypothesis transcript after each partial result and
    /// finishes when the recognizer reports `isFinal`.
    func transcribe() throws -> AsyncThrowingStream<String, Error> {
        guard let recognizer, recognizer.isAvailable else {
            throw SpeechError.unavailable
        }

        let request = SFSpeechAudioBufferRecognitionRequest()
        request.requiresOnDeviceRecognition = true
        request.addsPunctuation = true
        request.taskHint = .dictation
        request.customizedLanguageModel = FoodLanguageModel.configuration()
        self.request = request

        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.record, mode: .measurement, options: .duckOthers)
            try session.setActive(true, options: .notifyOthersOnDeactivation)
        } catch {
            throw SpeechError.audioEngineFailure(error.localizedDescription)
        }

        let input = audioEngine.inputNode
        let format = input.outputFormat(forBus: 0)
        input.removeTap(onBus: 0)
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak request] buffer, _ in
            request?.append(buffer)
        }

        audioEngine.prepare()
        do {
            try audioEngine.start()
        } catch {
            throw SpeechError.audioEngineFailure(error.localizedDescription)
        }

        return AsyncThrowingStream { continuation in
            let task = recognizer.recognitionTask(with: request) { result, error in
                if let error {
                    continuation.finish(throwing: SpeechError.recognizerFailure(error.localizedDescription))
                    return
                }
                if let result {
                    continuation.yield(result.bestTranscription.formattedString)
                    if result.isFinal {
                        continuation.finish()
                    }
                }
            }
            self.task = task

            continuation.onTermination = { @Sendable _ in
                Task { await self.stop() }
            }
        }
    }

    func stop() {
        if audioEngine.isRunning {
            audioEngine.stop()
            audioEngine.inputNode.removeTap(onBus: 0)
        }
        request?.endAudio()
        task?.cancel()
        request = nil
        task = nil
    }
}
