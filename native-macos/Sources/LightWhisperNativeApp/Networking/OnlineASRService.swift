import Foundation

enum OnlineASRError: LocalizedError {
    case missingAPIKey(String)
    case invalidResponse
    case requestFailed(String)

    var errorDescription: String? {
        switch self {
        case .missingAPIKey(let provider):
            return "\(provider) API key is not configured."
        case .invalidResponse:
            return "The ASR service returned an unreadable response."
        case .requestFailed(let message):
            return message
        }
    }
}

enum OnlineASRService {
    static func transcribe(
        audioWAV: Data,
        settings: EngineSettings,
        apiKey: String,
        hotWords: [String]
    ) async throws -> String {
        switch settings.engine {
        case .glmAsr:
            return try await GLMASRClient().transcribe(
                audioWAV: audioWAV,
                settings: settings,
                apiKey: apiKey,
                hotWords: hotWords
            )
        case .alibabaAsr:
            return try await AlibabaASRClient().transcribe(
                audioWAV: audioWAV,
                settings: settings,
                apiKey: apiKey,
                hotWords: hotWords
            )
        }
    }
}

private struct GLMASRClient {
    func transcribe(
        audioWAV: Data,
        settings: EngineSettings,
        apiKey: String,
        hotWords: [String]
    ) async throws -> String {
        let apiKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !apiKey.isEmpty else {
            throw OnlineASRError.missingAPIKey("GLM-ASR")
        }

        let boundary = "LightWhisperBoundary-\(UUID().uuidString)"
        var request = URLRequest(url: URL(string: "\(settings.activeEndpointBaseURL)/api/paas/v4/audio/transcriptions")!)
        request.httpMethod = "POST"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        request.timeoutInterval = 30
        request.httpBody = MultipartBody(boundary: boundary)
            .addFileField(name: "file", filename: "audio.wav", mimeType: "audio/wav", data: audioWAV)
            .addTextField(name: "model", value: "glm-asr-2512")
            .addTextField(name: "stream", value: "false")
            .addOptionalTextField(name: "hotwords", value: hotWords.isEmpty ? nil : try? JSONEncoder().encodeStringArray(hotWords))
            .build()

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw OnlineASRError.invalidResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw OnlineASRError.requestFailed("GLM-ASR HTTP \(httpResponse.statusCode): \(String(decoding: data, as: UTF8.self))")
        }

        let decoded = try JSONDecoder().decode(GLMResponse.self, from: data)
        if let code = decoded.code, code != 0 {
            throw OnlineASRError.requestFailed(decoded.message ?? "GLM-ASR returned error code \(code)")
        }
        return decoded.text?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    }

    private struct GLMResponse: Decodable {
        let text: String?
        let code: Int?
        let message: String?
    }
}

private struct AlibabaASRClient {
    func transcribe(
        audioWAV: Data,
        settings: EngineSettings,
        apiKey: String,
        hotWords _: [String]
    ) async throws -> String {
        let apiKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !apiKey.isEmpty else {
            throw OnlineASRError.missingAPIKey("Alibaba DashScope")
        }

        if settings.alibabaModelUsesOmniChat {
            return try await transcribeViaOmni(audioWAV: audioWAV, settings: settings, apiKey: apiKey)
        }
        return try await transcribeViaGeneration(audioWAV: audioWAV, settings: settings, apiKey: apiKey)
    }

    private func transcribeViaGeneration(
        audioWAV: Data,
        settings: EngineSettings,
        apiKey: String
    ) async throws -> String {
        let audioDataURL = "data:audio/wav;base64,\(audioWAV.base64EncodedString())"
        let body: [String: Any] = [
            "model": settings.alibabaModel,
            "input": [
                "messages": [
                    ["role": "system", "content": [["text": ""]]],
                    ["role": "user", "content": [["audio": audioDataURL]]],
                ],
            ],
            "parameters": [
                "asr_options": [
                    "enable_itn": true,
                ],
            ],
        ]

        var request = URLRequest(
            url: URL(string: "\(settings.activeEndpointBaseURL)/api/v1/services/aigc/multimodal-generation/generation")!
        )
        request.httpMethod = "POST"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.timeoutInterval = 60
        request.httpBody = try JSONSerialization.data(withJSONObject: body)

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw OnlineASRError.invalidResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw OnlineASRError.requestFailed(
                "DashScope HTTP \(httpResponse.statusCode): \(String(decoding: data, as: UTF8.self))"
            )
        }

        let decoded = try JSONDecoder().decode(DashScopeResponse.self, from: data)
        if let code = decoded.code, !code.isEmpty, code != "Success" {
            throw OnlineASRError.requestFailed(decoded.message ?? "DashScope ASR returned \(code)")
        }

        return decoded.output?.choices?.first?.message?.contentText() ?? ""
    }

    private func transcribeViaOmni(
        audioWAV: Data,
        settings: EngineSettings,
        apiKey: String
    ) async throws -> String {
        let audioDataURL = "data:;base64,\(audioWAV.base64EncodedString())"
        let body: [String: Any] = [
            "model": settings.alibabaModel,
            "stream": true,
            "stream_options": ["include_usage": false],
            "modalities": ["text"],
            "messages": [
                [
                    "role": "system",
                    "content": "You are a professional speech recognizer. Transcribe the audio verbatim. Output only the transcription.",
                ],
                [
                    "role": "user",
                    "content": [
                        ["type": "input_audio", "input_audio": ["data": audioDataURL, "format": "wav"]],
                        ["type": "text", "text": "Please transcribe this audio into text. Return the transcription only."],
                    ],
                ],
            ],
        ]

        var request = URLRequest(
            url: URL(string: "\(settings.activeEndpointBaseURL)/compatible-mode/v1/chat/completions")!
        )
        request.httpMethod = "POST"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        request.timeoutInterval = 60
        request.httpBody = try JSONSerialization.data(withJSONObject: body)

        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw OnlineASRError.invalidResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw OnlineASRError.requestFailed("DashScope Omni HTTP \(httpResponse.statusCode)")
        }

        var collected = ""
        for try await line in bytes.lines {
            guard line.hasPrefix("data: ") else {
                continue
            }
            let payload = String(line.dropFirst(6)).trimmingCharacters(in: .whitespacesAndNewlines)
            if payload == "[DONE]" || payload.isEmpty {
                continue
            }
            guard let data = payload.data(using: .utf8) else {
                continue
            }
            let chunk = try? JSONDecoder().decode(OmniChunk.self, from: data)
            chunk?.choices?.forEach { choice in
                if let delta = choice.delta {
                    collected += delta.contentString()
                }
            }
        }

        return collected.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private struct DashScopeResponse: Decodable {
        let code: String?
        let message: String?
        let output: Output?

        struct Output: Decodable {
            let choices: [Choice]?
        }

        struct Choice: Decodable {
            let message: Message?
        }

        struct Message: Decodable {
            let content: [Content]

            func contentText() -> String {
                content.compactMap(\.text).joined()
            }
        }

        struct Content: Decodable {
            let text: String?
        }
    }

    private struct OmniChunk: Decodable {
        let choices: [Choice]?

        struct Choice: Decodable {
            let delta: Delta?
        }

        struct Delta: Decodable {
            let content: ContentValue?

            func contentString() -> String {
                content?.stringValue ?? ""
            }
        }

        enum ContentValue: Decodable {
            case string(String)
            case array([ContentPart])

            var stringValue: String {
                switch self {
                case .string(let value):
                    return value
                case .array(let items):
                    return items.compactMap(\.text).joined()
                }
            }

            init(from decoder: Decoder) throws {
                let singleValue = try decoder.singleValueContainer()
                if let value = try? singleValue.decode(String.self) {
                    self = .string(value)
                } else {
                    self = .array(try singleValue.decode([ContentPart].self))
                }
            }
        }

        struct ContentPart: Decodable {
            let text: String?
        }
    }
}

private struct MultipartBody {
    let boundary: String
    private var parts = Data()

    init(boundary: String) {
        self.boundary = boundary
    }

    func addTextField(name: String, value: String) -> MultipartBody {
        var copy = self
        copy.parts.append("--\(boundary)\r\n".data(using: .utf8)!)
        copy.parts.append("Content-Disposition: form-data; name=\"\(name)\"\r\n\r\n".data(using: .utf8)!)
        copy.parts.append("\(value)\r\n".data(using: .utf8)!)
        return copy
    }

    func addOptionalTextField(name: String, value: String?) -> MultipartBody {
        guard let value else {
            return self
        }
        return addTextField(name: name, value: value)
    }

    func addFileField(name: String, filename: String, mimeType: String, data: Data) -> MultipartBody {
        var copy = self
        copy.parts.append("--\(boundary)\r\n".data(using: .utf8)!)
        copy.parts.append(
            "Content-Disposition: form-data; name=\"\(name)\"; filename=\"\(filename)\"\r\n".data(using: .utf8)!
        )
        copy.parts.append("Content-Type: \(mimeType)\r\n\r\n".data(using: .utf8)!)
        copy.parts.append(data)
        copy.parts.append("\r\n".data(using: .utf8)!)
        return copy
    }

    func build() -> Data {
        var body = parts
        body.append("--\(boundary)--\r\n".data(using: .utf8)!)
        return body
    }
}

private extension JSONEncoder {
    func encodeStringArray(_ value: [String]) throws -> String {
        let data = try encode(value)
        return String(decoding: data, as: UTF8.self)
    }
}
