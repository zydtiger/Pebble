import SwiftUI

enum PebbleTheme {
    static let canvas = Color(red: 0.975, green: 0.967, blue: 0.947)
    static let sidebar = Color(red: 0.935, green: 0.927, blue: 0.897)
    static let ink = Color(red: 0.22, green: 0.26, blue: 0.24)
    static let muted = Color(red: 0.48, green: 0.50, blue: 0.46)
    static let sage = Color(red: 0.35, green: 0.46, blue: 0.39)
    static let line = Color(red: 0.86, green: 0.86, blue: 0.82)
    static let clay = Color(red: 0.77, green: 0.60, blue: 0.48)
}

struct PebbleMark: View {
    var size: CGFloat = 40
    var body: some View {
        ZStack {
            Ellipse().fill(PebbleTheme.sage)
                .frame(width: size * 0.71, height: size * 0.51)
                .rotationEffect(.degrees(-24))
                .offset(x: -size * 0.13, y: size * 0.10)
            Ellipse().fill(PebbleTheme.clay)
                .frame(width: size * 0.50, height: size * 0.39)
                .rotationEffect(.degrees(26))
                .offset(x: size * 0.22, y: -size * 0.13)
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

struct CharacterAvatar: View {
    var size: CGFloat = 40
    var body: some View {
        ZStack {
            Circle().fill(Color(red: 0.85, green: 0.88, blue: 0.79))
            PebbleMark(size: size * 0.64)
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

struct SoftButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 12, weight: .medium))
            .padding(.horizontal, 15).padding(.vertical, 11)
            .background(.white.opacity(configuration.isPressed ? 0.4 : 0.8), in: Capsule())
            .overlay(Capsule().stroke(PebbleTheme.line.opacity(0.6), lineWidth: 1))
            .foregroundStyle(PebbleTheme.ink)
    }
}

struct SidebarActionButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled
    @State private var hovering = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .opacity(isEnabled ? (hovering || configuration.isPressed ? 0.55 : 1) : 0.4)
            .onHover { hovering = $0 }
    }
}
