import AppKit
import SwiftUI

@main
struct AvatarChecks {
    @MainActor
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let model = ChatModel()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 730),
                              styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.contentView = NSHostingView(rootView: ContentView(model: model))
        window.center()
        window.makeKeyAndOrderFront(nil)
        Task { @MainActor in
            do {
                try await settle(model)
                guard let first = model.selected else { throw CheckError.failed("Missing seeded persona") }
                try require(model.avatars.isEmpty, "Absent avatars must use placeholders")
                let path = URL(fileURLWithPath: first.directory).appendingPathComponent("avatar.png")
                try capture(window, name: "avatar-placeholder")

                try picture(center: .green).write(to: path, options: .atomic)
                await model.load()
                guard let green = model.avatars[first.id] else { throw CheckError.failed("Added avatar did not load") }
                try require(green.size == NSSize(width: 256, height: 128), "Preview must be bounded and retain aspect ratio")
                let rendered = ImageRenderer(content: CharacterAvatar(size: 64, image: green).padding(8).background(.white))
                guard let cg = rendered.cgImage else { throw CheckError.failed("Avatar did not render") }
                let bitmap = NSBitmapImageRep(cgImage: cg)
                let corner = bitmap.colorAt(x: 9, y: 9)!.usingColorSpace(.deviceRGB)!
                try require(corner.redComponent > 0.95 && corner.greenComponent > 0.95 && corner.blueComponent > 0.95,
                            "Circular avatar must leave the outside background intact")
                let edge = bitmap.colorAt(x: 10, y: 40)!.usingColorSpace(.deviceRGB)!
                try require(edge.greenComponent > 0.8 && edge.redComponent < 0.2,
                            "Wide image must center-crop, not stretch or letterbox")
                try await pause()
                try capture(window, name: "avatar-custom")
                model.showCharacter = true
                try await pause()
                guard let sheet = window.attachedSheet else { throw CheckError.failed("Character sheet missing") }
                try capture(sheet, name: "avatar-details")
                model.showCharacter = false
                try await pause()

                try picture(center: .blue).write(to: path, options: .atomic)
                await model.load()
                try require(center(model.avatars[first.id]!).blueComponent > 0.8, "Replacement remained stale")
                try FileManager.default.removeItem(at: path)
                await model.load()
                try require(model.avatars[first.id] == nil, "Removal kept a stale custom avatar")
                try Data("invalid PNG".utf8).write(to: path)
                await model.load()
                try require(model.avatars[first.id] == nil && model.error == nil, "Unreadable image must fall back without breaking chat")

                try picture(center: .green).write(to: path, options: .atomic)
                await model.load()
                try require(await model.createCharacter("Kai"), "Could not create second persona")
                try await settle(model)
                guard let second = model.selected else { throw CheckError.failed("Missing second persona") }
                try require(second.id != first.id && model.avatars[second.id] == nil, "New persona borrowed another avatar")
                let secondPath = URL(fileURLWithPath: second.directory).appendingPathComponent("avatar.png")
                try picture(center: .blue).write(to: secondPath, options: .atomic)
                await model.load()
                try require(center(model.avatars[first.id]!).greenComponent > 0.8 &&
                            center(model.avatars[second.id]!).blueComponent > 0.8, "Persona identities share a preview")
                var classifier = try await model.readClassifierSettings(nil)
                classifier.connection.enabled = false
                try await model.saveClassifierSettings(nil, update: classifier, credential: .keep)
                try require(await model.createGroup("Avatar check", members: [first.id, second.id]), "Group creation failed")
                try await settle(model)
                model.mentionedIDs = [first.id, second.id]
                model.draft = "Hello, both of you."
                model.sendMessage()
                for _ in 0..<500 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(20)) }
                try require(!model.hasActiveReplies && model.error == nil &&
                            Set(model.bubbles.filter { !$0.isUser }.map(\.senderID)) == Set([first.id, second.id]),
                            "Group fixture must contain both actual authors")
                try await pause()
                try capture(window, name: "avatar-group")
                window.setContentSize(NSSize(width: 820, height: 610))
                try await pause()
                try capture(window, name: "avatar-group-compact")
                model.select(first.id)
                try await settle(model)
                model.sendMessage("Hello again.")
                for _ in 0..<500 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(20)) }
                try require(!model.hasActiveReplies && model.error == nil, "Private demo reply did not finish")
                try await pause()
                try capture(window, name: "avatar-private")
                model.closeChat()
                print("PASS: absent, added, replaced, removed, invalid, bounded/cropped, per-persona and private/group avatar presentation")
                app.terminate(nil)
            } catch {
                model.closeChat()
                print("FAIL: \(error)")
                exit(1)
            }
        }
        app.run()
    }

    @MainActor
    static func settle(_ model: ChatModel) async throws {
        try await pause()
        for _ in 0..<200 where model.loading { try await Task.sleep(for: .milliseconds(20)) }
        try require(!model.loading && model.error == nil, "Workspace did not load: \(model.error ?? "still loading")")
    }
    static func pause() async throws { try await Task.sleep(for: .milliseconds(250)) }
    static func require(_ condition: Bool, _ message: String) throws {
        if !condition { throw CheckError.failed(message) }
    }
    @MainActor
    static func picture(center: NSColor) throws -> Data {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 1024, pixelsHigh: 512,
                                     bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                                     isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
        NSColor.red.setFill()
        NSRect(x: 0, y: 0, width: 1024, height: 512).fill()
        center.setFill()
        NSRect(x: 256, y: 0, width: 512, height: 512).fill()
        NSGraphicsContext.restoreGraphicsState()
        guard let data = bitmap.representation(using: .png, properties: [:]) else { throw CheckError.failed("PNG fixture failed") }
        return data
    }
    @MainActor
    static func center(_ image: NSImage) -> NSColor {
        let bitmap = NSBitmapImageRep(cgImage: image.cgImage(forProposedRect: nil, context: nil, hints: nil)!)
        return bitmap.colorAt(x: bitmap.pixelsWide / 2, y: bitmap.pixelsHigh / 2)!.usingColorSpace(.deviceRGB)!
    }
    @MainActor
    static func capture(_ window: NSWindow, name: String) throws {
        guard let view = window.contentView,
              let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { throw CheckError.failed("No bitmap") }
        view.layoutSubtreeIfNeeded()
        view.cacheDisplay(in: view.bounds, to: bitmap)
        guard let png = bitmap.representation(using: .png, properties: [:]) else { throw CheckError.failed("No PNG") }
        try png.write(to: URL(fileURLWithPath: "build/validation/persona-avatars/\(name).png"))
    }
    enum CheckError: Error { case failed(String) }
}
