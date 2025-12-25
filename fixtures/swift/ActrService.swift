import Foundation
import SwiftUI

@MainActor
final class ActrService: ObservableObject {
    @Published var connectionStatus: ConnectionStatus = .disconnected
    @Published var errorMessage: String?

    func initialize() async throws {
        connectionStatus = .error
        let message = "ActrService is not generated. Run `actr gen -l swift -i protos/echo.proto -o {{PROJECT_NAME_PASCAL}}/Generated`."
        errorMessage = message
        throw NSError(domain: "ActrService", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
    }

    func sendEcho(_ message: String) async throws -> String {
        let hint = "ActrService is not generated. Run `actr gen -l swift -i protos/echo.proto -o {{PROJECT_NAME_PASCAL}}/Generated`."
        errorMessage = hint
        throw NSError(domain: "ActrService", code: 2, userInfo: [NSLocalizedDescriptionKey: hint])
    }

    func shutdown() async {
        connectionStatus = .disconnected
        errorMessage = nil
    }
}

extension ActrService {
    enum ConnectionStatus {
        case disconnected
        case initializing
        case connected
        case error

        var description: String {
            switch self {
            case .disconnected:
                return "Disconnected"
            case .initializing:
                return "Initializing"
            case .connected:
                return "Connected"
            case .error:
                return "Error"
            }
        }
    }
}
