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
    var image: NSImage?
    var body: some View {
        ZStack {
            Circle().fill(Color(red: 0.85, green: 0.88, blue: 0.79))
            if let image {
                Image(nsImage: image).resizable().scaledToFill()
                    .frame(width: size, height: size).clipShape(Circle())
            } else {
                PebbleMark(size: size * 0.64)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

struct GroupAvatar: View {
    let memberIDs: [String]
    let images: [String: NSImage]
    var size: CGFloat = 38

    // Coordinates are in portrait diameters, with staggered rows spaced by sqrt(3)/2.
    private var centers: [CGPoint] {
        switch min(memberIDs.count, 7) {
        case 0: []
        case 1: [.zero]
        case 2: [CGPoint(x: -0.32, y: -0.32), CGPoint(x: 0.32, y: 0.32)]
        case 3: [CGPoint(x: 0, y: -0.433), CGPoint(x: -0.5, y: 0.433), CGPoint(x: 0.5, y: 0.433)]
        case 4: [CGPoint(x: -0.75, y: -0.433), CGPoint(x: 0.25, y: -0.433),
                 CGPoint(x: -0.25, y: 0.433), CGPoint(x: 0.75, y: 0.433)]
        case 5: [CGPoint(x: -0.5, y: -0.433), CGPoint(x: 0.5, y: -0.433),
                 CGPoint(x: -1, y: 0.433), CGPoint(x: 0, y: 0.433), CGPoint(x: 1, y: 0.433)]
        case 6: [CGPoint(x: -0.5, y: -0.866), CGPoint(x: 0.5, y: -0.866),
                 CGPoint(x: -1, y: 0), CGPoint(x: 1, y: 0),
                 CGPoint(x: -0.5, y: 0.866), CGPoint(x: 0.5, y: 0.866)]
        default: [.zero, CGPoint(x: -0.5, y: -0.866), CGPoint(x: 0.5, y: -0.866),
                  CGPoint(x: 1, y: 0), CGPoint(x: 0.5, y: 0.866),
                  CGPoint(x: -0.5, y: 0.866), CGPoint(x: -1, y: 0)]
        }
    }

    var body: some View {
        let points = centers
        let extent = max(points.map { abs($0.x) }.max() ?? 0, points.map { abs($0.y) }.max() ?? 0)
        let diameter = size / (extent * 2 + 1)
        ZStack {
            if memberIDs.isEmpty {
                Image(systemName: "person.2")
                    .font(.system(size: size * 0.6, weight: .light))
                    .foregroundStyle(PebbleTheme.sage)
            }
            ForEach(Array(memberIDs.prefix(7).enumerated()), id: \.element) { index, id in
                CharacterAvatar(size: diameter, image: images[id])
                    .offset(x: points[index].x * diameter, y: points[index].y * diameter)
            }
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
