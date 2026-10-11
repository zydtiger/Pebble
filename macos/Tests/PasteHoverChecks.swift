import AppKit
import SwiftUI

@MainActor
private final class ClipboardFixture {
    private let board = NSPasteboard.general
    private let retained: [[(NSPasteboard.PasteboardType, Data)]]

    init() {
        retained = (board.pasteboardItems ?? []).map { item in
            item.types.compactMap { type in item.data(forType: type).map { (type, $0) } }
        }
    }

    func set(_ data: Data, type: NSPasteboard.PasteboardType) {
        board.clearContents()
        board.setData(data, forType: type)
    }

    func restore() {
        board.clearContents()
        board.writeObjects(retained.map { entries in
            let item = NSPasteboardItem()
            for (type, data) in entries { item.setData(data, forType: type) }
            return item
        })
    }
}

@main
struct PasteHoverChecks {
    enum Failure: Error { case check(String) }

    @MainActor static func require(_ value: @autoclosure () -> Bool, _ detail: String) throws {
        if !value() { throw Failure.check(detail) }
    }

    @MainActor static func wait(seconds: Int = 10, _ condition: () -> Bool) async throws {
        for _ in 0..<(seconds * 100) {
            if condition() { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        throw Failure.check("Native Paste checkpoint timed out")
    }

    @MainActor static func editor(_ view: NSView) -> ComposerTextView? {
        if let text = view as? ComposerTextView { return text }
        return view.subviews.lazy.compactMap { editor($0) }.first
    }

    @MainActor static func capture(_ window: NSWindow, _ directory: URL, _ name: String) throws {
        let view = window.contentView!
        view.layoutSubtreeIfNeeded()
        guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds),
              let _ = bitmap.bitmapData else { throw Failure.check("Window bitmap unavailable") }
        view.cacheDisplay(in: view.bounds, to: bitmap)
        try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("\(name).png"))
    }

    @MainActor static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let menu = NSMenu()
        let edit = NSMenu(title: "Edit")
        let paste = NSMenuItem(title: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(paste)
        let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
        editItem.submenu = edit
        menu.addItem(editItem)
        app.mainMenu = menu
        let clipboard = ClipboardFixture()
        let root = URL(fileURLWithPath: ProcessInfo.processInfo.environment["PEBBLE_DATA_DIR"]!)
        let evidence = URL(fileURLWithPath: ProcessInfo.processInfo.environment["PEBBLE_PASTE_EVIDENCE"]!)
        let model = ChatModel(dataRoot: root)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 730), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Pebble Paste and Hover Checks"
        window.contentView = NSHostingView(rootView: ContentView(model: model))
        window.center()
        window.makeKeyAndOrderFront(nil)
        app.activate(ignoringOtherApps: true)
        Task { @MainActor in
            do {
                try await wait { model.selected != nil && !model.loading }
                guard let text = editor(window.contentView!) else { throw Failure.check("Composer unavailable") }
                window.makeFirstResponder(text)
                let picture = NSImage(size: NSSize(width: 160, height: 100))
                picture.lockFocus()
                NSColor.white.setFill(); NSRect(x: 0, y: 0, width: 160, height: 100).fill()
                NSColor.red.setFill(); NSBezierPath(ovalIn: NSRect(x: 30, y: 10, width: 80, height: 80)).fill()
                picture.unlockFocus()
                let tiff = picture.tiffRepresentation!
                let png = NSBitmapImageRep(data: tiff)!.representation(using: .png, properties: [:])!
                try png.write(to: root.appendingPathComponent("synthetic-circle.png"))
                let commandV = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .command,
                                               timestamp: 0, windowNumber: window.windowNumber, context: nil,
                                               characters: "v", charactersIgnoringModifiers: "v", isARepeat: false, keyCode: 9)!

                // Let AppKit auto-enable the actual menu against the first responder,
                // then dispatch its key equivalent, rather than calling the importer.
                for (type, data) in [(NSPasteboard.PasteboardType.png, png), (.tiff, tiff)] {
                    clipboard.set(data, type: type)
                    edit.update()
                    try require(edit.autoenablesItems && paste.isEnabled, "Image-only Paste menu disabled for \(type.rawValue)")
                    let previous = model.draftImages.count
                    try require(menu.performKeyEquivalent(with: commandV), "Cmd+V was not handled by the Edit menu")
                    try await wait { !model.importingImages && model.draftImages.count == previous + 1 }
                    try require(model.draft.isEmpty, "Image Paste inserted text into the composer")
                }
                clipboard.set(Data("Plain text, 中文 👋\nSecond line".utf8), type: .string)
                edit.update()
                try require(paste.isEnabled && menu.performKeyEquivalent(with: commandV), "Ordinary text Paste stopped working")
                try await wait { model.draft == "Plain text, 中文 👋\nSecond line" }

                clipboard.set(png, type: .png)
                text.isEditable = false
                edit.update()
                try require(!paste.isEnabled && !text.validateUserInterfaceItem(paste), "Read-only image Paste remained enabled")
                let retained = model.draftImages.map(\.id)
                text.paste(nil)
                try require(model.draftImages.map(\.id) == retained && !model.importingImages, "Direct read-only Paste imported an image")
                clipboard.set(Data("Must not paste".utf8), type: .string)
                text.paste(nil)
                try require(text.string == model.draft, "Direct read-only Paste changed the text")
                text.isEditable = true

                clipboard.set(Data(repeating: 0, count: ImageInput.byteLimit + 1), type: .png)
                edit.update()
                try require(paste.isEnabled && menu.performKeyEquivalent(with: commandV), "Oversized image Paste did not reach validation")
                try require(model.error?.contains("12 MiB") == true && model.draftImages.map(\.id) == retained, "Oversized Paste bypassed the limit or lost the draft")
                model.error = nil
                clipboard.set(png, type: .png)
                for count in 3...4 {
                    edit.update()
                    try require(menu.performKeyEquivalent(with: commandV), "Repeated image Paste not handled")
                    try await wait { !model.importingImages && model.draftImages.count == count }
                }
                edit.update()
                try require(menu.performKeyEquivalent(with: commandV), "Fifth image Paste not handled")
                try await wait { !model.importingImages && model.error != nil }
                try require(model.draftImages.count == 4, "Fifth image Paste changed the bounded draft")
                model.error = nil
                for image in model.draftImages { model.removeImage(image.id) }
                model.draft = "Native Cmd+V image fixture."
                try await Task.sleep(for: .milliseconds(150))
                try capture(window, evidence, "sidebar-normal")
                window.makeFirstResponder(text)
                clipboard.set(png, type: .png)
                print("CUA PASTE READY: PasteHoverChecks.app; composer focused; general clipboard contains only the synthetic PNG. Press Cmd+V.")
                fflush(stdout)
                try await wait(seconds: 180) { !model.importingImages && model.draftImages.count == 1 }
                try require(model.draft == "Native Cmd+V image fixture.", "Actual keyboard Paste changed the draft text")
                try await Task.sleep(for: .milliseconds(150))
                try capture(window, evidence, "native-keyboard-image-paste")
                print("CUA HOVER READY: image attached. Inspect sidebar hover; create finish-ui in the evidence directory when finished.")
                fflush(stdout)
                try await wait(seconds: 240) { FileManager.default.fileExists(atPath: evidence.appendingPathComponent("finish-ui").path) }
                try capture(window, evidence, "sidebar-hover-final")
                clipboard.restore()
                model.closeChat()
                print("Native Paste checks passed: auto-enabled Edit menu, responder Cmd+V PNG/TIFF/text, disabled direct/menu Paste, byte/count limits, actual keyboard image Paste and preserved draft. Clipboard restored.")
                app.terminate(nil)
            } catch {
                clipboard.restore()
                model.closeChat()
                FileHandle.standardError.write(Data("Native Paste check failed: \(error). Clipboard restored.\n".utf8))
                exit(1)
            }
        }
        app.run()
    }
}
