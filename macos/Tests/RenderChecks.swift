import AppKit
import SwiftUI

@main
struct RenderChecks {
    @MainActor
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let model = ChatModel()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 730),
                              styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Pebble — UI check"
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.contentView = NSHostingView(rootView: ContentView(model: model))
        window.center()
        window.makeKeyAndOrderFront(nil)
        Task { @MainActor in
            do {
                try await Task.sleep(for: .milliseconds(800))
                guard model.selected != nil, model.error == nil else {
                    throw RenderError.failed(model.error ?? "No persona loaded")
                }
                try capture(window, name: "home")
                guard let editor = findEditor(window.contentView!) else {
                    throw RenderError.failed("Missing native text editor")
                }
                window.makeFirstResponder(editor)
                editor.setMarkedText("ni", selectedRange: NSRange(location: 2, length: 0),
                                     replacementRange: NSRange(location: NSNotFound, length: 0))
                guard editor.hasMarkedText(), model.bubbles.isEmpty else {
                    throw RenderError.failed("Composition unexpectedly sent a preview")
                }
                editor.insertText("你好", replacementRange: NSRange(location: NSNotFound, length: 0))
                editor.insertNewline(nil)
                try await Task.sleep(for: .milliseconds(100))
                guard model.draft == "你好\n", model.bubbles.isEmpty else {
                    throw RenderError.failed("Native composition/newline failed: \(model.draft.debugDescription)")
                }
                editor.setSelectedRange(NSRange(location: 0, length: 2))
                guard editor.selectedRange().length == 2 else {
                    throw RenderError.failed("Native input selection failed")
                }
                model.draft = ""
                model.startPreview("Hi, Mira. Tell me a little about yourself.")
                for _ in 0..<150 where model.isPreviewing { try await Task.sleep(for: .milliseconds(30)) }
                guard !model.isPreviewing, model.error == nil else { throw RenderError.failed("Preview did not finish") }
                try await Task.sleep(for: .milliseconds(300))
                try capture(window, name: "conversation")
                window.setContentSize(NSSize(width: 820, height: 610))
                try await Task.sleep(for: .milliseconds(300))
                try capture(window, name: "compact")
                print("Rendered actual SwiftUI views at 1000×730 and 820×610; native marked text, Chinese commit, newline, and input selection passed.")
                model.closePreview()
                app.terminate(nil)
            } catch {
                FileHandle.standardError.write(Data("UI render failed: \(error)\n".utf8))
                exit(1)
            }
        }
        app.run()
    }

    @MainActor
    static func findEditor(_ view: NSView) -> NSTextView? {
        if let editor = view as? NSTextView { return editor }
        return view.subviews.lazy.compactMap { findEditor($0) }.first
    }

    @MainActor
    static func capture(_ window: NSWindow, name: String) throws {
        guard let view = window.contentView,
              let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
            throw RenderError.failed("Could not create window bitmap")
        }
        view.layoutSubtreeIfNeeded()
        view.cacheDisplay(in: view.bounds, to: bitmap)
        guard let png = bitmap.representation(using: .png, properties: [:]) else {
            throw RenderError.failed("Could not encode window bitmap")
        }
        try png.write(to: URL(fileURLWithPath: "build/previews/\(name).png"))
    }
}

enum RenderError: Error { case failed(String) }
