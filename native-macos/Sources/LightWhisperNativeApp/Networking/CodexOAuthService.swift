import AppKit
import CryptoKit
import Foundation
import Network

struct CodexOAuthSession: Codable, Equatable, Sendable {
    var idToken: String
    var accessToken: String
    var refreshToken: String
    var apiKey: String
    var expiresAtMS: UInt64?
    var accountID: String?
    var email: String?
    var planType: String?

    enum CodingKeys: String, CodingKey {
        case idToken = "id_token"
        case accessToken = "access_token"
        case refreshToken = "refresh_token"
        case apiKey = "api_key"
        case expiresAtMS = "expires_at_ms"
        case accountID = "account_id"
        case email
        case planType = "plan_type"
    }

    init(
        idToken: String = "",
        accessToken: String = "",
        refreshToken: String = "",
        apiKey: String = "",
        expiresAtMS: UInt64? = nil,
        accountID: String? = nil,
        email: String? = nil,
        planType: String? = nil
    ) {
        self.idToken = idToken
        self.accessToken = accessToken
        self.refreshToken = refreshToken
        self.apiKey = apiKey
        self.expiresAtMS = expiresAtMS
        self.accountID = accountID
        self.email = email
        self.planType = planType
    }
}

struct CodexOAuthStatus: Equatable, Sendable {
    var loggedIn: Bool
    var email: String?
    var planType: String?
    var accountID: String?
    var expiresAtMS: UInt64?

    static let loggedOut = CodexOAuthStatus(
        loggedIn: false,
        email: nil,
        planType: nil,
        accountID: nil,
        expiresAtMS: nil
    )
}

struct ChatGPTBearerToken: Codable, Equatable, Sendable {
    var accessToken: String
    var accountID: String?

    enum CodingKeys: String, CodingKey {
        case accessToken = "access_token"
        case accountID = "account_id"
    }
}

private struct PersistedCodexOAuthSessionMeta: Codable, Equatable, Sendable {
    var expiresAtMS: UInt64?
    var accountID: String?
    var email: String?
    var planType: String?

    enum CodingKeys: String, CodingKey {
        case expiresAtMS = "expires_at_ms"
        case accountID = "account_id"
        case email
        case planType = "plan_type"
    }
}

private struct CodexJWTClaims: Decodable {
    var exp: UInt64?
    var email: String?
    var profile: CodexJWTProfile?
    var auth: CodexJWTAuth?

    enum CodingKeys: String, CodingKey {
        case exp
        case email
        case profile = "https://api.openai.com/profile"
        case auth = "https://api.openai.com/auth"
    }
}

private struct CodexJWTProfile: Decodable {
    var email: String?
}

private struct CodexJWTAuth: Decodable {
    var chatgptAccountID: String?
    var chatgptPlanType: String?

    enum CodingKeys: String, CodingKey {
        case chatgptAccountID = "chatgpt_account_id"
        case chatgptPlanType = "chatgpt_plan_type"
    }
}

private struct CodexTokenResponse: Decodable {
    var idToken: String?
    var accessToken: String
    var refreshToken: String?
    var expiresIn: UInt64?

    enum CodingKeys: String, CodingKey {
        case idToken = "id_token"
        case accessToken = "access_token"
        case refreshToken = "refresh_token"
        case expiresIn = "expires_in"
    }
}

private struct CodexTokenExchangeResponse: Decodable {
    var accessToken: String

    enum CodingKeys: String, CodingKey {
        case accessToken = "access_token"
    }
}

private struct OAuthCallback: Sendable {
    let code: String
    let channel: OAuthCallbackChannel
}

private final class OAuthCallbackChannel: @unchecked Sendable {
    private let connection: NWConnection
    private let queue: DispatchQueue
    private var hasResponded = false

    init(connection: NWConnection, queue: DispatchQueue) {
        self.connection = connection
        self.queue = queue
    }

    func respond(statusLine: String, html: String, completion: @escaping @Sendable (Error?) -> Void) {
        queue.async {
            guard !self.hasResponded else {
                completion(nil)
                return
            }

            self.hasResponded = true
            let payload = makeHTTPResponse(statusLine: statusLine, html: html)
            self.connection.send(content: payload, completion: .contentProcessed { error in
                self.connection.cancel()
                completion(error)
            })
        }
    }

    func respond(statusLine: String, html: String) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            respond(statusLine: statusLine, html: html) { error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume()
                }
            }
        }
    }
}

private final class CallbackStartState: @unchecked Sendable {
    var resumed = false
}

private final class OAuthCallbackServer: @unchecked Sendable {
    private let listener: NWListener
    private let queue = DispatchQueue(label: "com.light-whisper.codex-oauth.callback")
    private var continuation: CheckedContinuation<OAuthCallback, Error>?
    private var expectedState: String?
    private var finished = false

    var port: UInt16 {
        listener.port?.rawValue ?? 0
    }

    init(listener: NWListener) {
        self.listener = listener
    }

    static func bind(preferredPort: UInt16) async throws -> OAuthCallbackServer {
        do {
            return try await start(on: preferredPort)
        } catch {
            return try await start(on: nil)
        }
    }

    private static func start(on port: UInt16?) async throws -> OAuthCallbackServer {
        let endpointPort = port.flatMap(NWEndpoint.Port.init(rawValue:)) ?? .any
        let listener = try NWListener(using: .tcp, on: endpointPort)
        let server = OAuthCallbackServer(listener: listener)
        try await server.start()
        return server
    }

    private func start() async throws {
        try await withCheckedThrowingContinuation { continuation in
            let startState = CallbackStartState()
            listener.stateUpdateHandler = { [weak self] state in
                guard let self else { return }
                switch state {
                case .ready:
                    guard !startState.resumed else { return }
                    startState.resumed = true
                    continuation.resume()
                case .failed(let error):
                    guard !startState.resumed else { return }
                    startState.resumed = true
                    self.listener.cancel()
                    continuation.resume(throwing: error)
                case .cancelled:
                    guard !startState.resumed else { return }
                    startState.resumed = true
                    continuation.resume(throwing: CodexOAuthError.callbackServerCancelled)
                default:
                    break
                }
            }

            listener.newConnectionHandler = { [weak self] connection in
                self?.handle(connection)
            }

            listener.start(queue: queue)
        }
    }

    func waitForCallback(expectedState: String, timeout: TimeInterval) async throws -> OAuthCallback {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                guard self.continuation == nil, !self.finished else {
                    continuation.resume(throwing: CodexOAuthError.callbackAlreadyPending)
                    return
                }

                self.expectedState = expectedState
                self.continuation = continuation

                self.queue.asyncAfter(deadline: .now() + timeout) {
                    guard !self.finished, self.continuation != nil else { return }
                    self.complete(with: .failure(CodexOAuthError.callbackTimedOut))
                }
            }
        }
    }

    func abortWaiting(with error: Error) {
        queue.async {
            self.complete(with: .failure(error))
        }
    }

    func stop() {
        queue.async {
            self.listener.cancel()
        }
    }

    private func handle(_ connection: NWConnection) {
        guard !finished else {
            connection.cancel()
            return
        }

        connection.start(queue: queue)
        connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, _, error in
            guard let self else {
                connection.cancel()
                return
            }

            if let error {
                self.complete(with: .failure(CodexOAuthError.callbackReceiveFailed(error.localizedDescription)))
                connection.cancel()
                return
            }

            guard let data, let request = String(data: data, encoding: .utf8), !request.isEmpty else {
                let channel = OAuthCallbackChannel(connection: connection, queue: self.queue)
                channel.respond(
                    statusLine: "400 Bad Request",
                    html: makeCallbackHTML(title: "Invalid Callback", message: "OAuth callback request was empty.", autoClose: false)
                ) { _ in
                    self.complete(with: .failure(CodexOAuthError.invalidCallbackRequest))
                }
                return
            }

            self.processRequest(request, connection: connection)
        }
    }

    private func processRequest(_ request: String, connection: NWConnection) {
        let channel = OAuthCallbackChannel(connection: connection, queue: queue)
        let requestLine = request.components(separatedBy: "\r\n").first ?? request
        let parts = requestLine.split(separator: " ")
        guard parts.count >= 2 else {
            channel.respond(
                statusLine: "400 Bad Request",
                html: makeCallbackHTML(title: "Invalid Callback", message: "OAuth callback request line was malformed.", autoClose: false)
            ) { _ in
                self.complete(with: .failure(CodexOAuthError.invalidCallbackRequest))
            }
            return
        }

        let target = String(parts[1])
        guard let components = URLComponents(string: "http://localhost\(target)") else {
            channel.respond(
                statusLine: "400 Bad Request",
                html: makeCallbackHTML(title: "Invalid Callback", message: "OAuth callback URL could not be parsed.", autoClose: false)
            ) { _ in
                self.complete(with: .failure(CodexOAuthError.invalidCallbackRequest))
            }
            return
        }

        guard components.path == CodexOAuthService.callbackPath else {
            channel.respond(
                statusLine: "404 Not Found",
                html: makeCallbackHTML(title: "Invalid Callback", message: "Unexpected callback path.", autoClose: false)
            ) { _ in
                self.complete(with: .failure(CodexOAuthError.callbackPathMismatch))
            }
            return
        }

        let query = Dictionary(uniqueKeysWithValues: (components.queryItems ?? []).map { ($0.name, $0.value ?? "") })
        let expectedState = self.expectedState ?? ""
        let receivedState = query["state"] ?? ""
        guard receivedState == expectedState else {
            channel.respond(
                statusLine: "400 Bad Request",
                html: makeCallbackHTML(title: "State Mismatch", message: "Login state does not match the original request.", autoClose: false)
            ) { _ in
                self.complete(with: .failure(CodexOAuthError.stateMismatch))
            }
            return
        }

        if let errorCode = query["error"]?.trimmedOrNil {
            let message = oauthErrorMessage(errorCode: errorCode, errorDescription: query["error_description"])
            channel.respond(
                statusLine: "200 OK",
                html: makeCallbackHTML(title: "Authorization Failed", message: message, autoClose: false)
            ) { _ in
                self.complete(with: .failure(CodexOAuthError.loginFailed(message)))
            }
            return
        }

        guard let code = query["code"]?.trimmedOrNil else {
            channel.respond(
                statusLine: "400 Bad Request",
                html: makeCallbackHTML(title: "Missing Code", message: "Authorization code was not returned.", autoClose: false)
            ) { _ in
                self.complete(with: .failure(CodexOAuthError.missingAuthorizationCode))
            }
            return
        }

        complete(with: .success(OAuthCallback(code: code, channel: channel)))
    }

    private func complete(with result: Result<OAuthCallback, Error>) {
        guard !finished else { return }
        finished = true
        let continuation = self.continuation
        self.continuation = nil
        self.expectedState = nil
        listener.cancel()
        continuation?.resume(with: result)
    }
}

enum CodexOAuthService {
    static let openAIProvider = "openai"
    static let originator = "codex_cli_rs"
    static let chatGPTBearerUserAgent = "codex-cli"
    static let chatGPTCodexResponsesURL = "https://chatgpt.com/backend-api/codex/responses"

    static let sessionKeychainUser = "openai-codex-oauth"
    static let sessionRefreshTokenKeychainUser = "openai-codex-oauth-refresh-token"

    private static let clientID = "app_EMoamEEZ73f0CkXaXp7hrann"
    private static let issuer = "https://auth.openai.com"
    fileprivate static let callbackPath = "/auth/callback"
    private static let defaultCallbackPort: UInt16 = 1455
    private static let oauthTimeoutSeconds: TimeInterval = 5 * 60
    private static let refreshSkewSeconds: UInt64 = 60

    private static let chatGPTBearerPrefix = "openai-codex-chatgpt:"
    private static let oauthAPIKeyPrefix = "openai-codex-oauth-api-key:"

    static func sessionMetadataURL(fileManager: FileManager = .default) throws -> URL {
        try AppPaths.dataDirectory(fileManager: fileManager)
            .appendingPathComponent("openai_codex_oauth_session.json", isDirectory: false)
    }

    static func loadSession(
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default
    ) throws -> CodexOAuthSession? {
        let legacySession = try loadLegacySession(keychainStore: keychainStore)
        let refreshToken = try keychainStore.string(for: sessionRefreshTokenKeychainUser)?.trimmedOrNil

        if let refreshToken {
            let meta = try loadMetadata(fileManager: fileManager)
            var session = legacySession ?? CodexOAuthSession()
            session.refreshToken = refreshToken
            session.expiresAtMS = meta.expiresAtMS ?? session.expiresAtMS
            session.accountID = meta.accountID ?? session.accountID
            session.email = meta.email ?? session.email
            session.planType = meta.planType ?? session.planType
            return enrich(session)
        }

        guard let legacySession else {
            return nil
        }
        return enrich(legacySession)
    }

    static func saveSession(
        _ session: CodexOAuthSession,
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default
    ) throws {
        let enriched = enrich(session)
        if let refreshToken = enriched.refreshToken.trimmedOrNil {
            try keychainStore.set(refreshToken, for: sessionRefreshTokenKeychainUser)
            try writeMetadata(enriched, fileManager: fileManager)
            try? keychainStore.deleteValue(for: sessionKeychainUser)
            return
        }

        let payload = try JSONEncoder().encode(enriched)
        try keychainStore.set(String(decoding: payload, as: UTF8.self), for: sessionKeychainUser)
        try? keychainStore.deleteValue(for: sessionRefreshTokenKeychainUser)
        try removeMetadata(fileManager: fileManager)
    }

    static func status(
        sessionOverride: CodexOAuthSession? = nil,
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default
    ) -> CodexOAuthStatus {
        let session: CodexOAuthSession?
        if let sessionOverride {
            session = enrich(sessionOverride)
        } else {
            session = try? loadSession(keychainStore: keychainStore, fileManager: fileManager)
        }

        guard let session else {
            return .loggedOut
        }

        return CodexOAuthStatus(
            loggedIn: true,
            email: session.email?.trimmedOrNil,
            planType: session.planType?.trimmedOrNil,
            accountID: session.accountID?.trimmedOrNil,
            expiresAtMS: session.expiresAtMS
        )
    }

    static func login(
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default,
        session: URLSession = .shared
    ) async throws -> CodexOAuthStatus {
        let newSession = try await loginSession(
            keychainStore: keychainStore,
            fileManager: fileManager,
            session: session
        )
        return status(sessionOverride: newSession)
    }

    static func loginSession(
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default,
        session: URLSession = .shared
    ) async throws -> CodexOAuthSession {
        let callbackServer = try await OAuthCallbackServer.bind(preferredPort: defaultCallbackPort)
        defer { callbackServer.stop() }

        let redirectURI = "http://localhost:\(callbackServer.port)\(callbackPath)"
        let (codeVerifier, codeChallenge) = generatePKCEPair()
        let stateToken = generateState()
        let authorizeURL = try buildAuthorizeURL(
            redirectURI: redirectURI,
            codeChallenge: codeChallenge,
            state: stateToken
        )

        let callbackTask = Task {
            try await callbackServer.waitForCallback(
                expectedState: stateToken,
                timeout: oauthTimeoutSeconds
            )
        }
        await Task.yield()

        do {
            try openBrowser(authorizeURL)
        } catch {
            callbackServer.abortWaiting(with: error)
            callbackTask.cancel()
            throw error
        }

        let callback = try await callbackTask.value

        let tokenResponse: CodexTokenResponse
        do {
            tokenResponse = try await exchangeCodeForTokens(
                code: callback.code,
                redirectURI: redirectURI,
                codeVerifier: codeVerifier,
                session: session
            )
        } catch {
            try? await callback.channel.respond(
                statusLine: "200 OK",
                html: makeCallbackHTML(title: "Authorization Failed", message: error.localizedDescription, autoClose: false)
            )
            throw error
        }

        guard let idToken = tokenResponse.idToken?.trimmedOrNil else {
            let error = CodexOAuthError.missingIDToken
            try? await callback.channel.respond(
                statusLine: "200 OK",
                html: makeCallbackHTML(title: "Authorization Failed", message: error.localizedDescription, autoClose: false)
            )
            throw error
        }

        guard let refreshToken = tokenResponse.refreshToken?.trimmedOrNil else {
            let error = CodexOAuthError.missingRefreshToken
            try? await callback.channel.respond(
                statusLine: "200 OK",
                html: makeCallbackHTML(title: "Authorization Failed", message: error.localizedDescription, autoClose: false)
            )
            throw error
        }

        let apiKey: String
        do {
            apiKey = try await exchangeIDTokenForAPIKey(idToken: idToken, session: session)
        } catch {
            apiKey = ""
        }

        var oauthSession = CodexOAuthSession(
            idToken: idToken,
            accessToken: tokenResponse.accessToken,
            refreshToken: refreshToken,
            apiKey: apiKey,
            expiresAtMS: tokenResponse.expiresIn.map { nowMS().saturatingAdding($0.saturatingMultiplied(by: 1000)) },
            accountID: nil,
            email: nil,
            planType: nil
        )
        oauthSession = enrich(oauthSession)

        do {
            try saveSession(oauthSession, keychainStore: keychainStore, fileManager: fileManager)
        } catch {
            try? await callback.channel.respond(
                statusLine: "200 OK",
                html: makeCallbackHTML(title: "Authorization Failed", message: error.localizedDescription, autoClose: false)
            )
            throw error
        }

        try? await callback.channel.respond(
            statusLine: "200 OK",
            html: makeCallbackHTML(title: "Authorization Successful", message: "可以关闭这个页面并返回轻语。", autoClose: true)
        )

        return oauthSession
    }

    static func refreshSessionIfNeeded(
        sessionOverride: CodexOAuthSession? = nil,
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default,
        session: URLSession = .shared
    ) async throws -> CodexOAuthSession? {
        guard var oauthSession = try (sessionOverride ?? loadSession(keychainStore: keychainStore, fileManager: fileManager)) else {
            return nil
        }

        if !sessionNeedsRefresh(oauthSession),
           oauthSession.apiKey.trimmedOrNil != nil || oauthSession.accessToken.trimmedOrNil != nil
        {
            return oauthSession
        }

        let needsRehydration = oauthSession.idToken.trimmedOrNil == nil || oauthSession.accessToken.trimmedOrNil == nil
        if sessionNeedsRefresh(oauthSession) || needsRehydration {
            guard let refreshToken = oauthSession.refreshToken.trimmedOrNil else {
                throw CodexOAuthError.refreshTokenMissingForRefresh
            }

            let tokenResponse = try await refreshTokens(refreshToken: refreshToken, session: session)
            oauthSession = CodexOAuthSession(
                idToken: tokenResponse.idToken?.trimmedOrNil ?? oauthSession.idToken,
                accessToken: tokenResponse.accessToken,
                refreshToken: tokenResponse.refreshToken?.trimmedOrNil ?? refreshToken,
                apiKey: "",
                expiresAtMS: tokenResponse.expiresIn.map { nowMS().saturatingAdding($0.saturatingMultiplied(by: 1000)) },
                accountID: oauthSession.accountID,
                email: oauthSession.email,
                planType: oauthSession.planType
            )
        }

        oauthSession.apiKey = (try? await exchangeIDTokenForAPIKey(idToken: oauthSession.idToken, session: session)) ?? ""
        oauthSession = enrich(oauthSession)
        try saveSession(oauthSession, keychainStore: keychainStore, fileManager: fileManager)
        return oauthSession
    }

    static func logout(
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default
    ) throws {
        try? keychainStore.deleteValue(for: sessionKeychainUser)
        try? keychainStore.deleteValue(for: sessionRefreshTokenKeychainUser)
        try removeMetadata(fileManager: fileManager)
    }

    static func effectiveAuthMode(
        config: LLMProviderConfig,
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default
    ) -> OpenAIAuthMode {
        if let explicit = config.openAIAuthMode {
            return explicit
        }
        let session = try? loadSession(keychainStore: keychainStore, fileManager: fileManager)
        return session == nil ? .apiKey : .oauth
    }

    static func resolveProviderAPIKey(
        provider: String,
        config: LLMProviderConfig,
        manualAPIKey: String?,
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default
    ) throws -> String {
        try resolveProviderAPIKeyWithoutRefresh(
            provider: provider,
            config: config,
            manualAPIKey: manualAPIKey,
            keychainStore: keychainStore,
            fileManager: fileManager
        )
    }

    static func resolveProviderAPIKey(
        provider: String,
        config: LLMProviderConfig,
        manualAPIKey: String?,
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default,
        session: URLSession = .shared
    ) async throws -> String {
        if provider != openAIProvider {
            return try resolveProviderAPIKeyWithoutRefresh(
                provider: provider,
                config: config,
                manualAPIKey: manualAPIKey,
                keychainStore: keychainStore,
                fileManager: fileManager
            )
        }

        switch effectiveAuthMode(config: config, keychainStore: keychainStore, fileManager: fileManager) {
        case .apiKey:
            return try resolveProviderAPIKeyWithoutRefresh(
                provider: provider,
                config: config,
                manualAPIKey: manualAPIKey,
                keychainStore: keychainStore,
                fileManager: fileManager
            )
        case .oauth:
            guard let oauthSession = try await refreshSessionIfNeeded(
                keychainStore: keychainStore,
                fileManager: fileManager,
                session: session
            ) else {
                return ""
            }

            if let apiKey = oauthSession.apiKey.trimmedOrNil {
                return encodeOAuthAPIKey(apiKey) ?? ""
            }
            guard let accessToken = oauthSession.accessToken.trimmedOrNil else {
                return ""
            }
            return encodeChatGPTBearerToken(
                ChatGPTBearerToken(
                    accessToken: accessToken,
                    accountID: oauthSession.accountID?.trimmedOrNil
                )
            ) ?? ""
        }
    }

    private static func resolveProviderAPIKeyWithoutRefresh(
        provider: String,
        config: LLMProviderConfig,
        manualAPIKey: String?,
        keychainStore: KeychainStore,
        fileManager: FileManager
    ) throws -> String {
        if provider != openAIProvider {
            if let manualAPIKey = manualAPIKey?.trimmedOrNil {
                return manualAPIKey
            }
            return try keychainStore.string(for: LLMProviderCatalog.keychainUser(for: provider))?.trimmedOrNil ?? ""
        }

        switch effectiveAuthMode(config: config, keychainStore: keychainStore, fileManager: fileManager) {
        case .apiKey:
            if let manualAPIKey = manualAPIKey?.trimmedOrNil {
                return manualAPIKey
            }
            return try keychainStore.string(for: LLMProviderCatalog.keychainUser(for: provider))?.trimmedOrNil ?? ""
        case .oauth:
            guard let session = try loadSession(keychainStore: keychainStore, fileManager: fileManager) else {
                return ""
            }
            if let apiKey = session.apiKey.trimmedOrNil {
                return encodeOAuthAPIKey(apiKey) ?? ""
            }
            guard let accessToken = session.accessToken.trimmedOrNil else {
                return ""
            }
            return encodeChatGPTBearerToken(
                ChatGPTBearerToken(
                    accessToken: accessToken,
                    accountID: session.accountID?.trimmedOrNil
                )
            ) ?? ""
        }
    }

    static func encodeChatGPTBearerToken(_ token: ChatGPTBearerToken) -> String? {
        guard !token.accessToken.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return nil
        }
        guard let payload = try? JSONEncoder().encode(token) else {
            return nil
        }
        return "\(chatGPTBearerPrefix)\(Base64URL.encode(payload))"
    }

    static func decodeChatGPTBearerToken(_ input: String) -> ChatGPTBearerToken? {
        let payload = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let encoded = payload.stripPrefix(chatGPTBearerPrefix) else {
            return nil
        }
        guard let data = Base64URL.decode(encoded) else {
            return nil
        }
        return try? JSONDecoder().decode(ChatGPTBearerToken.self, from: data)
    }

    static func encodeOAuthAPIKey(_ apiKey: String) -> String? {
        guard let apiKey = apiKey.trimmedOrNil else {
            return nil
        }
        return "\(oauthAPIKeyPrefix)\(apiKey)"
    }

    static func decodeOAuthAPIKey(_ input: String) -> String? {
        let payload = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let apiKey = payload.stripPrefix(oauthAPIKeyPrefix)?.trimmedOrNil else {
            return nil
        }
        return apiKey
    }

    static func isOAuthOriginAuth(_ input: String) -> Bool {
        decodeChatGPTBearerToken(input) != nil || decodeOAuthAPIKey(input) != nil
    }

    private static func loadLegacySession(keychainStore: KeychainStore) throws -> CodexOAuthSession? {
        guard let raw = try keychainStore.string(for: sessionKeychainUser)?.trimmedOrNil else {
            return nil
        }
        guard let data = raw.data(using: .utf8) else {
            return nil
        }
        return try? JSONDecoder().decode(CodexOAuthSession.self, from: data)
    }

    private static func loadMetadata(fileManager: FileManager) throws -> PersistedCodexOAuthSessionMeta {
        let store = JSONFileStore<PersistedCodexOAuthSessionMeta>(url: try sessionMetadataURL(fileManager: fileManager))
        return (try? store.load(defaultValue: PersistedCodexOAuthSessionMeta())) ?? PersistedCodexOAuthSessionMeta()
    }

    private static func writeMetadata(
        _ session: CodexOAuthSession,
        fileManager: FileManager
    ) throws {
        let store = JSONFileStore<PersistedCodexOAuthSessionMeta>(url: try sessionMetadataURL(fileManager: fileManager))
        try store.save(
            PersistedCodexOAuthSessionMeta(
                expiresAtMS: session.expiresAtMS,
                accountID: session.accountID?.trimmedOrNil,
                email: session.email?.trimmedOrNil,
                planType: session.planType?.trimmedOrNil
            )
        )
    }

    private static func removeMetadata(fileManager: FileManager) throws {
        let url = try sessionMetadataURL(fileManager: fileManager)
        if fileManager.fileExists(atPath: url.path) {
            try fileManager.removeItem(at: url)
        }
    }

    private static func openBrowser(_ url: URL) throws {
        guard NSWorkspace.shared.open(url) else {
            throw CodexOAuthError.browserOpenFailed("Failed to open the system browser.")
        }
    }

    private static func exchangeCodeForTokens(
        code: String,
        redirectURI: String,
        codeVerifier: String,
        session: URLSession
    ) async throws -> CodexTokenResponse {
        let endpoint = try tokenEndpointURL()
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        request.httpBody = formURLEncodedBody([
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirectURI),
            ("client_id", clientID),
            ("code_verifier", codeVerifier),
        ])

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw CodexOAuthError.invalidTokenResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw CodexOAuthError.tokenExchangeFailed(status: httpResponse.statusCode, body: String(decoding: data, as: UTF8.self))
        }

        return try JSONDecoder().decode(CodexTokenResponse.self, from: data)
    }

    private static func refreshTokens(
        refreshToken: String,
        session: URLSession
    ) async throws -> CodexTokenResponse {
        let endpoint = try tokenEndpointURL()
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        request.httpBody = formURLEncodedBody([
            ("grant_type", "refresh_token"),
            ("refresh_token", refreshToken),
            ("client_id", clientID),
        ])

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw CodexOAuthError.invalidTokenResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw CodexOAuthError.refreshFailed(status: httpResponse.statusCode, body: String(decoding: data, as: UTF8.self))
        }

        return try JSONDecoder().decode(CodexTokenResponse.self, from: data)
    }

    private static func exchangeIDTokenForAPIKey(
        idToken: String,
        session: URLSession
    ) async throws -> String {
        let endpoint = try tokenEndpointURL()
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        request.httpBody = formURLEncodedBody([
            ("grant_type", "urn:ietf:params:oauth:grant-type:token-exchange"),
            ("client_id", clientID),
            ("requested_token", "openai-api-key"),
            ("subject_token", idToken),
            ("subject_token_type", "urn:ietf:params:oauth:token-type:id_token"),
        ])

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw CodexOAuthError.invalidTokenResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw CodexOAuthError.apiKeyExchangeFailed(status: httpResponse.statusCode, body: String(decoding: data, as: UTF8.self))
        }

        return try JSONDecoder().decode(CodexTokenExchangeResponse.self, from: data).accessToken
    }

    private static func buildAuthorizeURL(
        redirectURI: String,
        codeChallenge: String,
        state: String
    ) throws -> URL {
        guard var components = URLComponents(string: "\(issuer)/oauth/authorize") else {
            throw CodexOAuthError.invalidAuthorizeURL
        }
        components.queryItems = [
            URLQueryItem(name: "response_type", value: "code"),
            URLQueryItem(name: "client_id", value: clientID),
            URLQueryItem(name: "redirect_uri", value: redirectURI),
            URLQueryItem(name: "scope", value: "openid profile email offline_access api.connectors.read api.connectors.invoke"),
            URLQueryItem(name: "code_challenge", value: codeChallenge),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
            URLQueryItem(name: "id_token_add_organizations", value: "true"),
            URLQueryItem(name: "codex_cli_simplified_flow", value: "true"),
            URLQueryItem(name: "state", value: state),
            URLQueryItem(name: "originator", value: originator),
        ]
        guard let url = components.url else {
            throw CodexOAuthError.invalidAuthorizeURL
        }
        return url
    }

    private static func generatePKCEPair() -> (verifier: String, challenge: String) {
        let verifierData = randomData(length: 64)
        let verifier = Base64URL.encode(verifierData)
        let challenge = Base64URL.encode(Data(SHA256.hash(data: Data(verifier.utf8))))
        return (verifier, challenge)
    }

    private static func generateState() -> String {
        Base64URL.encode(randomData(length: 48))
    }

    private static func sessionNeedsRefresh(_ session: CodexOAuthSession) -> Bool {
        guard let expiresAtMS = session.expiresAtMS else {
            return false
        }
        return expiresAtMS <= nowMS().saturatingAdding(refreshSkewSeconds.saturatingMultiplied(by: 1000))
    }

    private static func enrich(_ session: CodexOAuthSession) -> CodexOAuthSession {
        var session = session
        let claims = decodeJWTClaims(session.idToken) ?? decodeJWTClaims(session.accessToken)

        if let claims {
            if session.email?.trimmedOrNil == nil {
                session.email = claims.email?.trimmedOrNil ?? claims.profile?.email?.trimmedOrNil
            }
            if session.accountID?.trimmedOrNil == nil {
                session.accountID = claims.auth?.chatgptAccountID?.trimmedOrNil
            }
            if session.planType?.trimmedOrNil == nil {
                session.planType = claims.auth?.chatgptPlanType?.trimmedOrNil
            }
            if session.expiresAtMS == nil, let exp = claims.exp {
                session.expiresAtMS = exp.saturatingMultiplied(by: 1000)
            }
        }

        return session
    }

    private static func decodeJWTClaims(_ token: String) -> CodexJWTClaims? {
        let parts = token.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 3, let data = Base64URL.decode(String(parts[1])) else {
            return nil
        }
        return try? JSONDecoder().decode(CodexJWTClaims.self, from: data)
    }

    private static func tokenEndpointURL() throws -> URL {
        guard let url = URL(string: "\(issuer)/oauth/token") else {
            throw CodexOAuthError.invalidTokenEndpoint
        }
        return url
    }

    private static func formURLEncodedBody(_ pairs: [(String, String)]) -> Data {
        var components = URLComponents()
        components.queryItems = pairs.map(URLQueryItem.init)
        return Data((components.percentEncodedQuery ?? "").utf8)
    }

    private static func randomData(length: Int) -> Data {
        Data((0..<length).map { _ in UInt8.random(in: .min ... .max) })
    }

    private static func nowMS() -> UInt64 {
        let milliseconds = Date().timeIntervalSince1970 * 1000
        return milliseconds <= 0 ? 0 : UInt64(milliseconds.rounded(.down))
    }
}

private enum Base64URL {
    static func encode(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    static func decode(_ value: String) -> Data? {
        var base64 = value
            .replacingOccurrences(of: "-", with: "+")
            .replacingOccurrences(of: "_", with: "/")

        switch base64.count % 4 {
        case 0:
            break
        case 2:
            base64 += "=="
        case 3:
            base64 += "="
        default:
            return nil
        }

        return Data(base64Encoded: base64)
    }
}

private enum CodexOAuthError: LocalizedError {
    case browserOpenFailed(String)
    case invalidAuthorizeURL
    case invalidTokenEndpoint
    case invalidTokenResponse
    case tokenExchangeFailed(status: Int, body: String)
    case refreshFailed(status: Int, body: String)
    case apiKeyExchangeFailed(status: Int, body: String)
    case missingIDToken
    case missingRefreshToken
    case refreshTokenMissingForRefresh
    case callbackTimedOut
    case callbackAlreadyPending
    case callbackServerCancelled
    case callbackReceiveFailed(String)
    case callbackPathMismatch
    case stateMismatch
    case invalidCallbackRequest
    case missingAuthorizationCode
    case loginFailed(String)

    var errorDescription: String? {
        switch self {
        case .browserOpenFailed(let message):
            return "打开浏览器失败: \(message)"
        case .invalidAuthorizeURL:
            return "构造 OpenAI OAuth 地址失败。"
        case .invalidTokenEndpoint:
            return "OpenAI OAuth token 地址无效。"
        case .invalidTokenResponse:
            return "解析 OpenAI OAuth token 响应失败。"
        case .tokenExchangeFailed(let status, let body):
            return "OAuth 授权码换 token 失败 \(status): \(body)"
        case .refreshFailed(let status, let body):
            return "刷新 Codex OAuth token 失败 \(status): \(body)"
        case .apiKeyExchangeFailed(let status, let body):
            return "交换 OpenAI API Key 失败 \(status): \(body)"
        case .missingIDToken:
            return "OAuth 响应缺少 id_token，无法继续。"
        case .missingRefreshToken:
            return "OAuth 响应缺少 refresh_token，无法继续。"
        case .refreshTokenMissingForRefresh:
            return "OpenAI Codex OAuth 会话缺少 refresh token，请重新登录。"
        case .callbackTimedOut:
            return "等待 OpenAI OAuth 回调超时，请重试。"
        case .callbackAlreadyPending:
            return "OpenAI OAuth 回调已在等待中。"
        case .callbackServerCancelled:
            return "OpenAI OAuth 回调服务已取消。"
        case .callbackReceiveFailed(let message):
            return "读取 OAuth 回调失败: \(message)"
        case .callbackPathMismatch:
            return "OAuth 回调路径不正确。"
        case .stateMismatch:
            return "OpenAI OAuth state 校验失败，请重试。"
        case .invalidCallbackRequest:
            return "OAuth 回调请求格式不正确。"
        case .missingAuthorizationCode:
            return "OAuth 回调缺少 authorization code。"
        case .loginFailed(let message):
            return message
        }
    }
}

private func oauthErrorMessage(errorCode: String, errorDescription: String?) -> String {
    if errorCode == "access_denied",
       errorDescription?.lowercased().contains("missing_codex_entitlement") == true
    {
        return "当前 ChatGPT 工作区没有 Codex 权限，请联系管理员开通。"
    }

    if let description = errorDescription?.trimmedOrNil {
        return "OpenAI Codex OAuth 登录失败: \(description)"
    }

    return "OpenAI Codex OAuth 登录失败: \(errorCode)"
}

private func makeCallbackHTML(title: String, message: String, autoClose: Bool) -> String {
    let script = autoClose ? "<script>setTimeout(() => window.close(), 1200)</script>" : ""
    let escapedTitle = htmlEscape(title)
    let escapedMessage = htmlEscape(message)

    return """
    <!doctype html>
    <html>
    <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>\(escapedTitle)</title>
    </head>
    <body style="margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#111827;color:#f9fafb;font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;text-align:center;">
    <div style="max-width:680px;padding:48px 32px;display:flex;flex-direction:column;align-items:center;gap:18px;">
    <h1 style="margin:0;font-size:34px;line-height:1.2;font-weight:700;">\(escapedTitle)</h1>
    <p style="margin:0;font-size:21px;line-height:1.65;white-space:pre-wrap;">\(escapedMessage)</p>
    </div>
    \(script)
    </body>
    </html>
    """
}

private func makeHTTPResponse(statusLine: String, html: String) -> Data {
    let body = Data(html.utf8)
    let headers = """
    HTTP/1.1 \(statusLine)\r
    Content-Type: text/html; charset=utf-8\r
    Content-Length: \(body.count)\r
    Connection: close\r
    \r
    """

    var payload = Data(headers.utf8)
    payload.append(body)
    return payload
}

private func htmlEscape(_ input: String) -> String {
    var escaped = String()
    escaped.reserveCapacity(input.count)

    for character in input {
        switch character {
        case "&":
            escaped.append("&amp;")
        case "<":
            escaped.append("&lt;")
        case ">":
            escaped.append("&gt;")
        case "\"":
            escaped.append("&quot;")
        case "'":
            escaped.append("&#39;")
        default:
            escaped.append(character)
        }
    }

    return escaped
}

private extension String {
    var trimmedOrNil: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    func stripPrefix(_ prefix: String) -> String? {
        guard hasPrefix(prefix) else { return nil }
        return String(dropFirst(prefix.count))
    }
}

private extension UInt64 {
    func saturatingMultiplied(by value: UInt64) -> UInt64 {
        let result = multipliedReportingOverflow(by: value)
        return result.overflow ? UInt64.max : result.partialValue
    }

    func saturatingAdding(_ value: UInt64) -> UInt64 {
        let result = addingReportingOverflow(value)
        return result.overflow ? UInt64.max : result.partialValue
    }
}
