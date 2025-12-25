import OSLog
import SwiftUI

@main
struct {{APP_STRUCT_NAME}}: App {
    private let logger = Logger(subsystem: "{{BUNDLE_ID}}", category: "App")

    init() {
        logger.notice("{{PROJECT_NAME_PASCAL}} is initializing")
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .onAppear {
                    logger.notice("{{PROJECT_NAME_PASCAL}} view appeared")
                }
        }
    }
}
