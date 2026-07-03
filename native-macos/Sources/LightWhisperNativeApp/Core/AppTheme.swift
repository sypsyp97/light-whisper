import SwiftUI

enum InterfaceAccentRole: String, CaseIterable, Sendable {
    case rust
    case amber
    case moss
}

enum AppTheme {
    static let ink = Color(red: 0.06, green: 0.07, blue: 0.08)
    static let canvas = Color(red: 0.98, green: 0.98, blue: 0.96)
    static let line = Color(red: 0.82, green: 0.84, blue: 0.82)
    static let mutedText = Color(red: 0.38, green: 0.41, blue: 0.39)

    static func accent(_ role: InterfaceAccentRole) -> Color {
        switch role {
        case .rust:
            return Color(red: 0.76, green: 0.27, blue: 0.17)
        case .amber:
            return Color(red: 0.88, green: 0.58, blue: 0.17)
        case .moss:
            return Color(red: 0.25, green: 0.52, blue: 0.37)
        }
    }
}
