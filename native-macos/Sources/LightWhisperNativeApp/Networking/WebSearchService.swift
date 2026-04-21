import Foundation

struct SearchResult: Equatable, Sendable {
    let title: String
    let url: String
    let content: String
}

enum WebSearchService {
    private static let timeout: TimeInterval = 15

    static func exaSearch(
        query: String,
        maxResults: UInt8,
        session: URLSession = .shared
    ) async throws -> [SearchResult] {
        let body: [String: Any] = [
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": [
                "name": "web_search_exa",
                "arguments": [
                    "query": query,
                    "numResults": maxResults,
                    "type": "auto",
                ],
            ],
        ]

        var request = URLRequest(url: URL(string: "https://mcp.exa.ai/mcp")!)
        request.httpMethod = "POST"
        request.timeoutInterval = timeout
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json, text/event-stream", forHTTPHeaderField: "Accept")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw LLMTransportError.invalidResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw LLMTransportError.requestFailed("Exa HTTP \(httpResponse.statusCode): \(String(decoding: data, as: UTF8.self))")
        }

        let raw = String(decoding: data, as: UTF8.self)
        let jsonString: String
        if raw.contains("event:") {
            jsonString = raw.split(separator: "\n").reversed().compactMap { line in
                let value = String(line)
                return value.hasPrefix("data:") ? value.dropFirst(5).trimmingCharacters(in: .whitespaces) : nil
            }.first ?? raw
        } else {
            jsonString = raw
        }

        let responseObject = try JSONSerialization.jsonObject(with: Data(jsonString.utf8)) as? [String: Any]
        let blocks = ((responseObject?["result"] as? [String: Any])?["content"] as? [[String: Any]]) ?? []
        var results: [SearchResult] = []
        for block in blocks {
            let text = block["text"] as? String ?? ""
            for entry in text.components(separatedBy: "\n\n") {
                let parsed = parseExaTextBlock(entry)
                if !parsed.title.isEmpty || !parsed.url.isEmpty {
                    results.append(parsed)
                }
            }
        }
        return results
    }

    static func tavilySearch(
        apiKey: String,
        query: String,
        maxResults: UInt8,
        session: URLSession = .shared
    ) async throws -> [SearchResult] {
        var request = URLRequest(url: URL(string: "https://api.tavily.com/search")!)
        request.httpMethod = "POST"
        request.timeoutInterval = timeout
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.httpBody = try JSONSerialization.data(withJSONObject: [
            "query": query,
            "max_results": maxResults,
            "include_answer": false,
        ])

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw LLMTransportError.invalidResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw LLMTransportError.requestFailed("Tavily HTTP \(httpResponse.statusCode): \(String(decoding: data, as: UTF8.self))")
        }

        let decoded = try JSONDecoder().decode(TavilyResponse.self, from: data)
        return decoded.results.map { SearchResult(title: $0.title, url: $0.url, content: $0.content) }
    }

    static func renderContext(_ results: [SearchResult]) -> String {
        var output = "<web_search_results>\n"
        output += "<instruction>Use these results only when the user's request needs external or real-time information.</instruction>\n"
        for (index, result) in results.enumerated() {
            output += "<result index=\"\(index + 1)\">\n"
            output += PromptXML.wrap("title", result.title) + "\n"
            output += PromptXML.wrap("url", result.url) + "\n"
            output += PromptXML.wrap("content", result.content) + "\n"
            output += "</result>\n"
        }
        output += "</web_search_results>"
        return output
    }

    private static func parseExaTextBlock(_ block: String) -> SearchResult {
        var title = ""
        var url = ""
        var content = ""

        for line in block.split(separator: "\n") {
            let value = String(line)
            if let match = value.removingPrefix("Title: ") {
                title = match.trimmingCharacters(in: CharacterSet.whitespacesAndNewlines)
            } else if let match = value.removingPrefix("URL: ") {
                url = match.trimmingCharacters(in: CharacterSet.whitespacesAndNewlines)
            } else if let match = value.removingPrefix("Text: ") {
                content = match.trimmingCharacters(in: CharacterSet.whitespacesAndNewlines)
            }
        }

        return SearchResult(title: title, url: url, content: content)
    }
}

private extension String {
    func removingPrefix(_ prefix: String) -> String? {
        guard hasPrefix(prefix) else { return nil }
        return String(dropFirst(prefix.count))
    }
}

private struct TavilyResponse: Decodable {
    let results: [TavilyHit]
}

private struct TavilyHit: Decodable {
    let title: String
    let url: String
    let content: String
}
