import AVFoundation
import AudioToolbox
import Foundation

struct CapturedAudio {
    let pcmData: Data
    let wavData: Data
    let duration: TimeInterval
}

enum AudioCaptureError: LocalizedError {
    case alreadyRecording
    case notRecording
    case deviceConfigurationFailed(String)

    var errorDescription: String? {
        switch self {
        case .alreadyRecording:
            return "Recording is already in progress."
        case .notRecording:
            return "No recording is in progress."
        case .deviceConfigurationFailed(let message):
            return message
        }
    }
}

@MainActor
final class AudioCaptureService {
    private let engine = AVAudioEngine()
    private let audioDeviceCatalog = AudioDeviceCatalog()
    private var capturedSamples: [Int16] = []
    private var sampleRate: Double = 16_000
    private var startDate: Date?
    private var levelHandler: ((Float) -> Void)?
    private(set) var isRecording = false

    func start(
        preferredDeviceUID: String? = nil,
        levelHandler: ((Float) -> Void)? = nil
    ) async throws {
        guard !isRecording else {
            throw AudioCaptureError.alreadyRecording
        }
        try await PermissionsService.ensureMicrophoneAccess()

        let inputNode = engine.inputNode
        try configureInputDeviceIfNeeded(preferredDeviceUID: preferredDeviceUID, inputNode: inputNode)
        let format = inputNode.inputFormat(forBus: 0)
        sampleRate = format.sampleRate
        capturedSamples.removeAll(keepingCapacity: true)
        self.levelHandler = levelHandler

        inputNode.removeTap(onBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 2048, format: format) { [weak self] buffer, _ in
            self?.consume(buffer: buffer)
        }

        try engine.start()
        startDate = Date()
        isRecording = true
    }

    func stop() throws -> CapturedAudio {
        guard isRecording else {
            throw AudioCaptureError.notRecording
        }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        isRecording = false
        let duration = startDate.map { Date().timeIntervalSince($0) } ?? 0
        let pcmData = Data(capturedSamples.flatMap(\.littleEndianData))
        let wavData = WAVEEncoder.encode(samples: capturedSamples, sampleRate: Int(sampleRate))
        return CapturedAudio(pcmData: pcmData, wavData: wavData, duration: duration)
    }

    private func consume(buffer: AVAudioPCMBuffer) {
        guard let channelData = buffer.floatChannelData else {
            return
        }

        let frameCount = Int(buffer.frameLength)
        let channels = Int(buffer.format.channelCount)
        let channelPointers = UnsafeBufferPointer(start: channelData, count: channels)
        var peak: Float = 0

        for frame in 0..<frameCount {
            var mixed: Float = 0
            for channel in channelPointers {
                mixed += channel[frame]
            }
            mixed /= Float(max(channels, 1))
            peak = max(peak, abs(mixed))
            let clamped = max(-1, min(1, mixed))
            capturedSamples.append(Int16(clamped * Float(Int16.max)))
        }

        levelHandler?(peak)
    }

    private func configureInputDeviceIfNeeded(
        preferredDeviceUID: String?,
        inputNode: AVAudioInputNode
    ) throws {
        let selection = try audioDeviceCatalog.preferredInputDevice(requestedUID: preferredDeviceUID)
        guard let device = selection.resolvedDevice else {
            return
        }
        guard let audioUnit = inputNode.audioUnit else {
            throw AudioCaptureError.deviceConfigurationFailed("The input audio unit is unavailable.")
        }

        var deviceID = device.deviceID
        let status = AudioUnitSetProperty(
            audioUnit,
            kAudioOutputUnitProperty_CurrentDevice,
            kAudioUnitScope_Global,
            0,
            &deviceID,
            UInt32(MemoryLayout<AudioDeviceID>.size)
        )
        guard status == noErr else {
            throw AudioCaptureError.deviceConfigurationFailed(
                "Unable to configure the input device (\(UInt32(bitPattern: status)))."
            )
        }
    }
}

private enum WAVEEncoder {
    static func encode(samples: [Int16], sampleRate: Int) -> Data {
        let dataSize = samples.count * MemoryLayout<Int16>.size
        let fileSize = 36 + dataSize
        var data = Data()

        data.append("RIFF".data(using: .ascii)!)
        data.append(UInt32(fileSize).littleEndianData)
        data.append("WAVE".data(using: .ascii)!)
        data.append("fmt ".data(using: .ascii)!)
        data.append(UInt32(16).littleEndianData)
        data.append(UInt16(1).littleEndianData)
        data.append(UInt16(1).littleEndianData)
        data.append(UInt32(sampleRate).littleEndianData)
        data.append(UInt32(sampleRate * 2).littleEndianData)
        data.append(UInt16(2).littleEndianData)
        data.append(UInt16(16).littleEndianData)
        data.append("data".data(using: .ascii)!)
        data.append(UInt32(dataSize).littleEndianData)

        for sample in samples {
            data.append(sample.littleEndianData)
        }
        return data
    }
}

private extension FixedWidthInteger {
    var littleEndianData: Data {
        withUnsafeBytes(of: littleEndian) { Data($0) }
    }
}
