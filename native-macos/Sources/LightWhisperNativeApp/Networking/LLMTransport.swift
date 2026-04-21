import Foundation

enum LLMTransportError: LocalizedError {
    case invalidURL(String)
    case invalidResponse
    case requestFailed(String)
    case unsupportedFormat

    var errorDescription: String? {
        switch self {
        case .invalidURL(let value):
            return "Invalid LLM endpoint URL: \(value)"
        case .invalidResponse:
            return "The LLM service returned an unreadable response."
        case .requestFailed(let message):
            return message
        case .unsupportedFormat:
            return "The LLM format is not supported."
        }
    }
}

struct LLMTransportImageInput: Equatable, Sendable {
    let mimeType: String
    let dataBase64: String
}

enum LLMTransport {
    private static let openAIFastModeServiceTier = "priority"

    static func openAICompatBody(
        endpoint: LLMEndpoint,
        system: String,
        user: String,
        jsonOutput: Bool,
        apiKey: String,
        fastMode: Bool,
        images: [LLMTransportImageInput] = [],
        webSearch: Bool = false
    ) throws -> Data {
        let body: [String: Any]
        if usesResponsesAPI(endpoint) {
            var userContent: [[String: Any]] = [[
                "type": "input_text",
                "text": user,
            ]]
            for image in images {
                userContent.append([
                    "type": "input_image",
                    "image_url": "data:\(image.mimeType);base64,\(image.dataBase64)",
                ])
            }
            var payload: [String: Any] = [
                "model": endpoint.model,
                "instructions": system,
                "input": [
                    [
                        "role": "developer",
                        "content": [[
                            "type": "input_text",
                            "text": jsonOutput ? "Output json." : "Follow the system instructions exactly.",
                        ]],
                    ],
                    [
                        "role": "user",
                        "content": userContent,
                    ],
                ],
                "max_output_tokens": 4096,
                "stream": false,
            ]
            if jsonOutput {
                payload["text"] = ["format": ["type": "json_object"]]
            }
            if usesChatGPTBearer(apiKey: apiKey) {
                payload["store"] = false
            }
            if usesOpenAIOAuthOriginAuth(endpoint: endpoint, apiKey: apiKey), fastMode {
                payload["service_tier"] = openAIFastModeServiceTier
            }
            if webSearch {
                payload["tools"] = [["type": "web_search"]]
            }
            body = payload
        } else {
            let userContent: Any
            if images.isEmpty {
                userContent = user
            } else {
                userContent = [
                    [
                        "type": "text",
                        "text": user,
                    ],
                ] + images.map { image in
                    [
                        "type": "image_url",
                        "image_url": [
                            "url": "data:\(image.mimeType);base64,\(image.dataBase64)",
                        ],
                    ]
                }
            }
            var payload: [String: Any] = [
                "model": endpoint.model,
                "messages": [
                    ["role": "system", "content": system],
                    ["role": "user", "content": userContent],
                ],
                "stream": false,
            ]
            if jsonOutput {
                payload["response_format"] = ["type": "json_object"]
            }
            if usesOpenAIOAuthOriginAuth(endpoint: endpoint, apiKey: apiKey), fastMode {
                payload["service_tier"] = openAIFastModeServiceTier
            }
            if webSearch {
                payload["tools"] = [[
                    "type": "web_search_preview",
                    "web_search_preview": [:],
                ]]
            }
            body = payload
        }
        return try JSONSerialization.data(withJSONObject: body)
    }

    static func anthropicBody(
        endpoint: LLMEndpoint,
        system: String,
        user: String,
        jsonOutput: Bool,
        images: [LLMTransportImageInput] = [],
        webSearch: Bool = false
    ) throws -> Data {
        let userContent: [[String: Any]]
        if images.isEmpty {
            userContent = [[
                "type": "text",
                "text": user,
            ]]
        } else {
            userContent = [[
                "type": "text",
                "text": user,
            ]] + images.map { image in
                [
                    "type": "image",
                    "source": [
                        "type": "base64",
                        "media_type": image.mimeType,
                        "data": image.dataBase64,
                    ],
                ]
            }
        }
        var body: [String: Any] = [
            "model": endpoint.model,
            "system": system,
            "max_tokens": 4000,
            "messages": [
                [
                    "role": "user",
                    "content": userContent,
                ],
            ],
        ]

        if jsonOutput {
            body["metadata"] = ["response_format": "json_object"]
        }
        if webSearch {
            body["tools"] = [[
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 3,
            ]]
        }

        return try JSONSerialization.data(withJSONObject: body)
    }

    static func requestBody(
        endpoint: LLMEndpoint,
        system: String,
        user: String,
        jsonOutput: Bool,
        apiKey: String,
        fastMode: Bool,
        images: [LLMTransportImageInput] = [],
        webSearch: Bool = false
    ) throws -> Data {
        switch endpoint.apiFormat {
        case .openaiCompat:
            return try openAICompatBody(
                endpoint: endpoint,
                system: system,
                user: user,
                jsonOutput: jsonOutput,
                apiKey: apiKey,
                fastMode: fastMode,
                images: images,
                webSearch: webSearch
            )
        case .anthropic:
            return try anthropicBody(
                endpoint: endpoint,
                system: system,
                user: user,
                jsonOutput: jsonOutput,
                images: images,
                webSearch: webSearch
            )
        }
    }

    static func send(
        endpoint: LLMEndpoint,
        apiKey: String,
        system: String,
        user: String,
        jsonOutput: Bool,
        fastMode: Bool = false,
        images: [LLMTransportImageInput] = [],
        webSearch: Bool = false,
        session: URLSession = .shared
    ) async throws -> String {
        guard let url = URL(string: endpoint.apiURL) else {
            throw LLMTransportError.invalidURL(endpoint.apiURL)
        }

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 60
        request.httpBody = try requestBody(
            endpoint: endpoint,
            system: system,
            user: user,
            jsonOutput: jsonOutput,
            apiKey: apiKey,
            fastMode: fastMode,
            images: images,
            webSearch: webSearch
        )

        for (name, value) in authorizationHeaders(endpoint: endpoint, apiKey: apiKey) {
            request.setValue(value, forHTTPHeaderField: name)
        }

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw LLMTransportError.invalidResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw LLMTransportError.requestFailed("LLM HTTP \(httpResponse.statusCode): \(String(decoding: data, as: UTF8.self))")
        }

        return try extractText(from: data, format: endpoint.apiFormat)
    }

    static func extractText(from data: Data, format: ApiFormat) throws -> String {
        switch format {
        case .openaiCompat:
            if let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] {
                if let outputText = json["output_text"] as? String {
                    return outputText
                }
                if let output = json["output"] as? [[String: Any]] {
                    for item in output where (item["type"] as? String) == "message" {
                        if let content = item["content"] as? [[String: Any]] {
                            let fragments = content.compactMap {
                                ($0["text"] as? String)
                                    ?? (($0["text"] as? [String: Any])?["value"] as? String)
                            }
                            if !fragments.isEmpty {
                                return fragments.joined()
                            }
                        }
                    }
                }
                if let choices = json["choices"] as? [[String: Any]],
                   let first = choices.first,
                   let message = first["message"] as? [String: Any] {
                    if let content = message["content"] as? String {
                        return content
                    }
                    if let contentArray = message["content"] as? [[String: Any]] {
                        return contentArray.compactMap { $0["text"] as? String }.joined()
                    }
                }
            }
        case .anthropic:
            if let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
               let content = json["content"] as? [[String: Any]] {
                return content.compactMap { $0["text"] as? String }.joined()
            }
        }

        throw LLMTransportError.invalidResponse
    }

    private static func authorizationHeaders(
        endpoint: LLMEndpoint,
        apiKey: String
    ) -> [String: String] {
        switch endpoint.apiFormat {
        case .anthropic:
            return [
                "x-api-key": apiKey,
                "anthropic-version": "2023-06-01",
                "Content-Type": "application/json",
            ]
        case .openaiCompat:
            if let token = CodexOAuthService.decodeChatGPTBearerToken(apiKey) {
                var headers = [
                    "Authorization": "Bearer \(token.accessToken)",
                    "Content-Type": "application/json",
                    "originator": CodexOAuthService.originator,
                    "User-Agent": CodexOAuthService.chatGPTBearerUserAgent,
                ]
                if let accountID = token.accountID?.trimmingCharacters(in: .whitespacesAndNewlines), !accountID.isEmpty {
                    headers["ChatGPT-Account-ID"] = accountID
                }
                return headers
            }

            let bearer = CodexOAuthService.decodeOAuthAPIKey(apiKey) ?? apiKey
            return [
                "Authorization": "Bearer \(bearer)",
                "Content-Type": "application/json",
            ]
        }
    }

    private static func usesResponsesAPI(_ endpoint: LLMEndpoint) -> Bool {
        endpoint.apiFormat == .openaiCompat && endpoint.apiURL.localizedCaseInsensitiveContains("/v1/responses")
    }

    static func looksLikeImageInputUnsupportedError(_ message: String) -> Bool {
        let normalized = message.lowercased()
        return (normalized.contains("image") || normalized.contains("input_image") || normalized.contains("image_url"))
            && (normalized.contains("unsupported")
                || normalized.contains("not supported")
                || normalized.contains("invalid"))
    }

    static func looksLikeWebSearchUnsupportedError(_ message: String) -> Bool {
        let normalized = message.lowercased()
        let mentionsSearchTool =
            normalized.contains("web_search")
            || normalized.contains("web search")
            || normalized.contains("web_search_preview")
            || normalized.contains("web_search_20250305")
        return mentionsSearchTool
            && (normalized.contains("unsupported")
                || normalized.contains("not supported")
                || normalized.contains("unknown")
                || normalized.contains("invalid"))
    }

    private static func usesChatGPTBearer(apiKey: String) -> Bool {
        CodexOAuthService.decodeChatGPTBearerToken(apiKey) != nil
    }

    private static func usesOpenAIOAuthOriginAuth(endpoint: LLMEndpoint, apiKey: String) -> Bool {
        endpoint.provider == CodexOAuthService.openAIProvider
            && (CodexOAuthService.decodeChatGPTBearerToken(apiKey) != nil
                || CodexOAuthService.decodeOAuthAPIKey(apiKey) != nil)
    }
}
