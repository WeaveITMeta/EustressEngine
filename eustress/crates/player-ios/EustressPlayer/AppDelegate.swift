// =============================================================================
// Eustress Player iOS - App Delegate
// =============================================================================
// Entry point for the iOS application. Initializes the Rust/Bevy engine.
// =============================================================================

import UIKit

@main
class AppDelegate: UIResponder, UIApplicationDelegate {

    var window: UIWindow?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        // Initialize the Rust library
        eustress_player_main()
        return true
    }

    func application(
        _ app: UIApplication,
        open url: URL,
        options: [UIApplication.OpenURLOptionsKey: Any] = [:]
    ) -> Bool {
        // Handle eustress:// URLs
        if url.scheme == "eustress" {
            handleEustressURL(url)
            return true
        }
        return false
    }

    private func handleEustressURL(_ url: URL) {
        // Parse URL and load game
        // eustress://play/game-id
        guard let host = url.host else { return }
        
        if host == "play" {
            let gameId = url.pathComponents.dropFirst().first
            print("Loading game: \(gameId ?? "unknown")")
            // Pass to Rust via FFI
        }
    }
}

// FFI declaration for Rust entry point
@_silgen_name("eustress_player_main")
func eustress_player_main()
