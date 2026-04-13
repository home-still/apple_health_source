import Foundation

/// URLSession wrapper for communicating with the Rust Axum API.
actor APIClient {
    static let shared = APIClient()

    /// Base URL of the API server. Configure this in settings.
    var baseURL = URL(string: "https://health.lolzlab.com")!

    /// JWT token obtained after login/register. Persisted to UserDefaults.
    var token: String? {
        didSet { UserDefaults.standard.set(token, forKey: "api_jwt_token") }
    }

    /// Device ID obtained after device registration. Persisted to UserDefaults.
    var deviceId: UUID? {
        didSet {
            UserDefaults.standard.set(deviceId?.uuidString, forKey: "api_device_id")
        }
    }

    private init() {
        // Restore persisted auth state
        self.token = UserDefaults.standard.string(forKey: "api_jwt_token")
        if let idStr = UserDefaults.standard.string(forKey: "api_device_id") {
            self.deviceId = UUID(uuidString: idStr)
        }
    }

    func setBaseURL(_ url: URL) {
        self.baseURL = url
    }

    func clearAuth() {
        self.token = nil
        self.deviceId = nil
    }

    func setDeviceId(_ id: UUID) {
        self.deviceId = id
    }

    private let session = URLSession.shared
    private let encoder: JSONEncoder = {
        let e = JSONEncoder()
        e.dateEncodingStrategy = .iso8601
        return e
    }()
    private let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.dateDecodingStrategy = .iso8601
        return d
    }()

    // MARK: - Auth

    struct AuthRequest: Codable {
        let email: String
        let password: String
    }

    struct AuthResponse: Codable {
        let token: String
        let userId: UUID

        enum CodingKeys: String, CodingKey {
            case token
            case userId = "user_id"
        }
    }

    func register(email: String, password: String) async throws -> AuthResponse {
        let body = AuthRequest(email: email, password: password)
        let response: AuthResponse = try await post(path: "/auth/register", body: body)
        self.token = response.token
        return response
    }

    func login(email: String, password: String) async throws -> AuthResponse {
        let body = AuthRequest(email: email, password: password)
        let response: AuthResponse = try await post(path: "/auth/login", body: body)
        self.token = response.token
        return response
    }

    // MARK: - Device Registration

    func registerDevice(_ registration: DeviceRegistration) async throws -> DeviceResponse {
        try await post(path: "/api/v1/devices/register", body: registration)
    }

    // MARK: - Hash Check (two-phase sync)

    func checkHashes(_ request: HashCheckRequest) async throws -> HashCheckResponse {
        try await post(path: "/api/v1/health/check", body: request)
    }

    // MARK: - Sync

    func syncHealth(_ payload: SyncPayload) async throws -> SyncResponse {
        try await post(path: "/api/v1/health/sync", body: payload)
    }

    // MARK: - Workout Routes

    func syncWorkoutRoutes(_ payload: WorkoutRoutePayload) async throws {
        let _: RouteResponse = try await post(path: "/api/v1/health/workout-routes", body: payload)
    }

    // MARK: - Voice Meal Logging

    func parseMeal(text: String, mealType: String) async throws -> MealNutritionResponse {
        let body = MealParseRequest(text: text, mealType: mealType)
        return try await post(path: "/api/meals/parse", body: body)
    }

    func mealHistory(limit: Int = 50) async throws -> MealHistoryResponse {
        try await get(path: "/api/meals/history?limit=\(limit)")
    }

    // MARK: - Delete

    func deleteSamples(_ uuids: [String]) async throws {
        let body = DeleteRequest(hkUuids: uuids)
        let _: DeleteResponse = try await post(path: "/api/v1/health/delete", body: body)
    }

    // MARK: - HTTP

    private func post<Body: Encodable, Response: Decodable>(
        path: String,
        body: Body
    ) async throws -> Response {
        var request = URLRequest(url: baseURL.appendingPathComponent(path))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        if let token {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }

        request.httpBody = try encoder.encode(body)

        let (data, response) = try await session.data(for: request)

        guard let http = response as? HTTPURLResponse else {
            throw APIError.invalidResponse
        }

        guard (200..<300).contains(http.statusCode) else {
            let message = String(data: data, encoding: .utf8) ?? "unknown error"
            throw APIError.server(statusCode: http.statusCode, message: message)
        }

        return try decoder.decode(Response.self, from: data)
    }

    private func get<Response: Decodable>(path: String) async throws -> Response {
        guard let url = URL(string: path, relativeTo: baseURL) else {
            throw APIError.invalidResponse
        }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        if let token {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }

        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw APIError.invalidResponse
        }
        guard (200..<300).contains(http.statusCode) else {
            let message = String(data: data, encoding: .utf8) ?? "unknown error"
            throw APIError.server(statusCode: http.statusCode, message: message)
        }
        return try decoder.decode(Response.self, from: data)
    }
}

/// Response from the delete endpoint.
private struct DeleteResponse: Decodable {
    let deleted: Int
}

/// Response from the workout routes endpoint.
private struct RouteResponse: Decodable {
    let pointsSynced: Int
    enum CodingKeys: String, CodingKey {
        case pointsSynced = "points_synced"
    }
}

enum APIError: Error, LocalizedError {
    case invalidResponse
    case server(statusCode: Int, message: String)

    var errorDescription: String? {
        switch self {
        case .invalidResponse:
            return "Invalid server response"
        case .server(let code, let message):
            return "Server error \(code): \(message)"
        }
    }
}
