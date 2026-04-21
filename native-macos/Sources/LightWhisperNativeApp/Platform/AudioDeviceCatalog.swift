import CoreAudio
import Foundation

struct AudioInputDevice: Identifiable, Equatable, Sendable {
    let id: String
    let uid: String
    let deviceID: AudioDeviceID
    let name: String
    let channelCount: UInt32
    let nominalSampleRate: Double?
    let transportType: String
    let isDefault: Bool
}

struct AudioInputSelection: Equatable, Sendable {
    let requestedUID: String?
    let resolvedDevice: AudioInputDevice?
    let fellBackToDefault: Bool
}

enum AudioDeviceCatalogError: LocalizedError {
    case coreAudio(OSStatus)
    case deviceNotFound(String)

    var errorDescription: String? {
        switch self {
        case let .coreAudio(status):
            let code = UInt32(bitPattern: status)
            return "CoreAudio error \(code)"
        case let .deviceNotFound(uid):
            return "The audio input device \(uid) could not be found."
        }
    }
}

struct AudioDeviceCatalog {
    func inputDevices() throws -> [AudioInputDevice] {
        let defaultDeviceID = try defaultInputDeviceID()
        return try allDeviceIDs()
            .compactMap { deviceID in
                guard let device = try makeInputDevice(deviceID: deviceID, defaultDeviceID: defaultDeviceID) else {
                    return nil
                }
                return device
            }
            .sorted { lhs, rhs in
                if lhs.isDefault != rhs.isDefault {
                    return lhs.isDefault && !rhs.isDefault
                }
                return lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
            }
    }

    func defaultInputDevice() throws -> AudioInputDevice? {
        let defaultDeviceID = try defaultInputDeviceID()
        return try makeInputDevice(deviceID: defaultDeviceID, defaultDeviceID: defaultDeviceID)
    }

    func defaultInputDeviceUID() throws -> String? {
        try defaultInputDevice()?.uid
    }

    func preferredInputDevice(requestedUID: String?) throws -> AudioInputSelection {
        let devices = try inputDevices()
        guard let requestedUID = requestedUID?.trimmingCharacters(in: .whitespacesAndNewlines), !requestedUID.isEmpty else {
            return AudioInputSelection(
                requestedUID: nil,
                resolvedDevice: devices.first(where: \.isDefault) ?? devices.first,
                fellBackToDefault: false
            )
        }

        if let matchedDevice = devices.first(where: { $0.uid == requestedUID }) {
            return AudioInputSelection(
                requestedUID: requestedUID,
                resolvedDevice: matchedDevice,
                fellBackToDefault: false
            )
        }

        return AudioInputSelection(
            requestedUID: requestedUID,
            resolvedDevice: devices.first(where: \.isDefault) ?? devices.first,
            fellBackToDefault: true
        )
    }

    func setDefaultInputDevice(uid: String) throws {
        let devices = try inputDevices()
        guard let device = devices.first(where: { $0.uid == uid }) else {
            throw AudioDeviceCatalogError.deviceNotFound(uid)
        }
        try setDefaultInputDevice(deviceID: device.deviceID)
    }

    func setDefaultInputDevice(deviceID: AudioDeviceID) throws {
        var deviceID = deviceID
        try setProperty(
            objectID: AudioObjectID(kAudioObjectSystemObject),
            selector: kAudioHardwarePropertyDefaultInputDevice,
            scope: kAudioObjectPropertyScopeGlobal,
            element: kAudioObjectPropertyElementMain,
            value: &deviceID
        )
    }

    private func allDeviceIDs() throws -> [AudioDeviceID] {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )

        var dataSize: UInt32 = 0
        try check(AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &dataSize))

        let count = Int(dataSize) / MemoryLayout<AudioDeviceID>.size
        var devices = Array(repeating: AudioDeviceID(), count: count)
        try check(AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &dataSize, &devices))
        return devices
    }

    private func defaultInputDeviceID() throws -> AudioDeviceID {
        try readAudioDeviceIDProperty(
            objectID: AudioObjectID(kAudioObjectSystemObject),
            selector: kAudioHardwarePropertyDefaultInputDevice,
            scope: kAudioObjectPropertyScopeGlobal,
            element: kAudioObjectPropertyElementMain
        )
    }

    private func makeInputDevice(deviceID: AudioDeviceID, defaultDeviceID: AudioDeviceID) throws -> AudioInputDevice? {
        let channelCount = try inputChannelCount(deviceID: deviceID)
        guard channelCount > 0 else {
            return nil
        }

        let uid = try readStringProperty(
            objectID: deviceID,
            selector: kAudioDevicePropertyDeviceUID,
            scope: kAudioObjectPropertyScopeGlobal
        )
        let name = try readStringProperty(
            objectID: deviceID,
            selector: kAudioObjectPropertyName,
            scope: kAudioObjectPropertyScopeGlobal
        )

        let sampleRate = try? readFloat64Property(
            objectID: deviceID,
            selector: kAudioDevicePropertyNominalSampleRate,
            scope: kAudioObjectPropertyScopeGlobal,
            element: kAudioObjectPropertyElementMain
        )
        let transportType = (try? readUInt32Property(
            objectID: deviceID,
            selector: kAudioDevicePropertyTransportType,
            scope: kAudioObjectPropertyScopeGlobal,
            element: kAudioObjectPropertyElementMain
        )).map(Self.transportName(for:)) ?? "unknown"

        return AudioInputDevice(
            id: uid,
            uid: uid,
            deviceID: deviceID,
            name: name,
            channelCount: channelCount,
            nominalSampleRate: sampleRate,
            transportType: transportType,
            isDefault: deviceID == defaultDeviceID
        )
    }

    private func inputChannelCount(deviceID: AudioDeviceID) throws -> UInt32 {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: kAudioDevicePropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain
        )

        var dataSize: UInt32 = 0
        try check(AudioObjectGetPropertyDataSize(deviceID, &address, 0, nil, &dataSize))

        let rawBuffer = UnsafeMutableRawPointer.allocate(
            byteCount: Int(dataSize),
            alignment: MemoryLayout<AudioBufferList>.alignment
        )
        defer { rawBuffer.deallocate() }

        try check(AudioObjectGetPropertyData(deviceID, &address, 0, nil, &dataSize, rawBuffer))

        let bufferList = rawBuffer.bindMemory(to: AudioBufferList.self, capacity: 1)
        let audioBuffers = UnsafeMutableAudioBufferListPointer(bufferList)
        return audioBuffers.reduce(0) { partialResult, audioBuffer in
            partialResult + audioBuffer.mNumberChannels
        }
    }

    private func readStringProperty(
        objectID: AudioObjectID,
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope
    ) throws -> String {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain
        )
        var value: CFString?
        var dataSize = UInt32(MemoryLayout<CFString?>.size)
        try withUnsafeMutablePointer(to: &value) { pointer in
            try check(AudioObjectGetPropertyData(objectID, &address, 0, nil, &dataSize, pointer))
        }
        return (value as String?) ?? ""
    }

    private func readAudioDeviceIDProperty(
        objectID: AudioObjectID,
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope,
        element: AudioObjectPropertyElement
    ) throws -> AudioDeviceID {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: scope,
            mElement: element
        )
        var value = AudioDeviceID()
        var dataSize = UInt32(MemoryLayout<AudioDeviceID>.size)
        try withUnsafeMutablePointer(to: &value) { pointer in
            try check(AudioObjectGetPropertyData(objectID, &address, 0, nil, &dataSize, pointer))
        }
        return value
    }

    private func readUInt32Property(
        objectID: AudioObjectID,
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope,
        element: AudioObjectPropertyElement
    ) throws -> UInt32 {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: scope,
            mElement: element
        )
        var value = UInt32.zero
        var dataSize = UInt32(MemoryLayout<UInt32>.size)
        try withUnsafeMutablePointer(to: &value) { pointer in
            try check(AudioObjectGetPropertyData(objectID, &address, 0, nil, &dataSize, pointer))
        }
        return value
    }

    private func readFloat64Property(
        objectID: AudioObjectID,
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope,
        element: AudioObjectPropertyElement
    ) throws -> Float64 {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: scope,
            mElement: element
        )
        var value = Float64.zero
        var dataSize = UInt32(MemoryLayout<Float64>.size)
        try withUnsafeMutablePointer(to: &value) { pointer in
            try check(AudioObjectGetPropertyData(objectID, &address, 0, nil, &dataSize, pointer))
        }
        return value
    }

    private func setProperty(
        objectID: AudioObjectID,
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope,
        element: AudioObjectPropertyElement,
        value: inout AudioDeviceID
    ) throws {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: scope,
            mElement: element
        )
        try withUnsafeMutablePointer(to: &value) { pointer in
            try check(AudioObjectSetPropertyData(
                objectID,
                &address,
                0,
                nil,
                UInt32(MemoryLayout<AudioDeviceID>.size),
                pointer
            ))
        }
    }

    private func check(_ status: OSStatus) throws {
        guard status == noErr else {
            throw AudioDeviceCatalogError.coreAudio(status)
        }
    }

    private static func transportName(for transportType: UInt32) -> String {
        switch transportType {
        case kAudioDeviceTransportTypeBuiltIn:
            return "built-in"
        case kAudioDeviceTransportTypeUSB:
            return "usb"
        case kAudioDeviceTransportTypeBluetooth, kAudioDeviceTransportTypeBluetoothLE:
            return "bluetooth"
        case kAudioDeviceTransportTypeAggregate:
            return "aggregate"
        case kAudioDeviceTransportTypeVirtual:
            return "virtual"
        case kAudioDeviceTransportTypePCI:
            return "pci"
        case kAudioDeviceTransportTypeAirPlay:
            return "airplay"
        case kAudioDeviceTransportTypeHDMI:
            return "hdmi"
        case kAudioDeviceTransportTypeDisplayPort:
            return "displayport"
        default:
            return "unknown"
        }
    }
}
