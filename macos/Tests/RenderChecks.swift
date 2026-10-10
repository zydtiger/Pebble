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
                try await Task.sleep(for: .milliseconds(100))
                try capture(window, name: "composer-empty")
                let emptyCaret = editor.firstRect(forCharacterRange: NSRange(location: 0, length: 0), actualRange: nil)
                editor.insertText("Leave a little thought…", replacementRange: NSRange(location: NSNotFound, length: 0))
                try await Task.sleep(for: .milliseconds(100))
                let typedCaret = editor.firstRect(forCharacterRange: NSRange(location: 0, length: 0), actualRange: nil)
                guard abs(emptyCaret.minX - typedCaret.minX) < 1,
                      abs(emptyCaret.minY - typedCaret.minY) < 1 else {
                    throw RenderError.failed("The first line moved when typing into the empty composer")
                }
                try capture(window, name: "composer-typed")
                editor.setSelectedRange(NSRange(location: 0, length: editor.string.utf16.count))
                editor.deleteBackward(nil)
                editor.setMarkedText("ni", selectedRange: NSRange(location: 2, length: 0),
                                     replacementRange: NSRange(location: NSNotFound, length: 0))
                guard editor.hasMarkedText(), model.bubbles.isEmpty else {
                    throw RenderError.failed("Composition unexpectedly sent a message")
                }
                try pressReturn(editor, in: window)
                guard model.bubbles.isEmpty, !model.isReplying else {
                    throw RenderError.failed("Return sent a draft while composing marked text")
                }
                editor.insertText("你好", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
                try pressReturn(editor, in: window, modifiers: .command)
                try pressReturn(editor, in: window, modifiers: .shift)
                try await Task.sleep(for: .milliseconds(100))
                guard model.draft == "你好\n\n", model.bubbles.isEmpty else {
                    throw RenderError.failed("Command–Enter/Shift–Enter failed to insert new lines: \(model.draft.debugDescription)")
                }
                editor.setSelectedRange(NSRange(location: 0, length: 2))
                guard editor.selectedRange().length == 2 else {
                    throw RenderError.failed("Native input selection failed")
                }
                try await checkComposerSizing(window: window, model: model, editor: editor)
                try pressReturn(editor, in: window)
                guard model.draft.isEmpty, model.bubbles.isEmpty else {
                    throw RenderError.failed("Enter should not send an empty draft")
                }
                let prompt = "Hi, Mira. Tell me a little about yourself."
                editor.insertText(prompt, replacementRange: NSRange(location: 0, length: 0))
                try pressReturn(editor, in: window)
                try await Task.sleep(for: .milliseconds(100))
                guard model.isReplying, model.draft.isEmpty, model.bubbles.first?.text == prompt else {
                    throw RenderError.failed("Enter did not send the draft")
                }
                guard model.bubbles.last?.text.isEmpty == true else { throw RenderError.failed("Unvalidated candidate became visible") }
                try capture(window, name: "typing")
                for _ in 0..<150 where model.isReplying { try await Task.sleep(for: .milliseconds(30)) }
                guard !model.isReplying, model.error == nil else { throw RenderError.failed("Demo reply did not finish") }
                try await Task.sleep(for: .milliseconds(300))
                try capture(window, name: "conversation")
                window.setContentSize(NSSize(width: 820, height: 610))
                try await Task.sleep(for: .milliseconds(300))
                try capture(window, name: "compact")
                let miraID = model.selected!.id
                guard await model.createCharacter("Kai") else { throw RenderError.failed("Could not create a second character") }
                for _ in 0..<100 where model.loading { try await Task.sleep(for: .milliseconds(10)) }
                let kaiID = model.selected!.id
                model.sendMessage("Kai's concurrent thought")
                model.select(miraID)
                model.sendMessage("Mira's next thought")
                model.sendMessage("A queued followup")
                try await Task.sleep(for: .milliseconds(150))
                guard model.pendingCount(miraID) == 2, model.pendingCount(kaiID) == 1,
                      model.bubbles.last?.status == .queued else { throw RenderError.failed("Concurrent queue presentation was missing") }
                try capture(window, name: "queued-rooms")
                for _ in 0..<500 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(10)) }
                guard !model.hasActiveReplies, model.error == nil else { throw RenderError.failed("Concurrent preview replies did not finish") }
                try await Task.sleep(for: .milliseconds(100))
                try capture(window, name: "multi-persona")
                try "[defaults.chat]\nmodel='synthetic-live-model'\nbase_url='http://127.0.0.1:9/v1'\napi='chat_completions'\napi_key=''\n"
                    .write(to: model.dataRoot.appendingPathComponent("config.toml"), atomically: true, encoding: .utf8)
                await model.load()
                guard model.selected?.connectionLabel == "Live · synthetic-live-model",
                      model.selected?.contextNotice.isEmpty == false else {
                    throw RenderError.failed("Live mode or fresh-context notice was not exposed")
                }
                try await Task.sleep(for: .milliseconds(200))
                try capture(window, name: "live-context")
                try "[defaults.chat]\nmodel='synthetic-live-model'\nbase_url='http://127.0.0.1:9/v1'\napi='responses'\napi_key=''\n"
                    .write(to: model.dataRoot.appendingPathComponent("config.toml"), atomically: true, encoding: .utf8)
                await model.load()
                guard model.selected?.connectionLabel == "Live · synthetic-live-model · Responses" else {
                    throw RenderError.failed("Responses mode label was not exposed")
                }
                try await Task.sleep(for: .milliseconds(200))
                try capture(window, name: "responses-context")
                let saved = model.bubbles.map(\.id)
                let reset = await model.startFreshContext()
                guard reset, model.bubbles.map(\.id) == saved,
                      model.selected?.contextNotice.contains("Fresh model context") == true else {
                    throw RenderError.failed("Fresh context presentation changed saved history")
                }
                try await Task.sleep(for: .milliseconds(200))
                try capture(window, name: "fresh-context")
                model.sendMessage("Synthetic configured connection failure")
                for _ in 0..<300 where model.isReplying { try await Task.sleep(for: .milliseconds(10)) }
                guard !model.isReplying, model.bubbles.last?.canRetry == true else {
                    throw RenderError.failed("Failed latest reply did not expose Retry")
                }
                try await Task.sleep(for: .milliseconds(200))
                try capture(window, name: "failed-retry")
                window.setContentSize(NSSize(width: 600, height: 620))
                window.contentView = NSHostingView(rootView: SettingsSheet(model: model, target: SettingsTarget(personaID: nil)))
                try await Task.sleep(for: .milliseconds(400))
                try capture(window, name: "app-settings")
                window.setContentSize(NSSize(width: 600, height: 740))
                window.contentView = NSHostingView(rootView: SettingsSheet(model: model, target: SettingsTarget(personaID: model.selectedID)))
                try await Task.sleep(for: .milliseconds(400))
                try capture(window, name: "character-settings")
                window.setContentSize(NSSize(width: 700, height: 750))
                window.contentView = NSHostingView(rootView: StateSheet(model: model, personaID: model.selectedID!))
                try await Task.sleep(for: .milliseconds(400))
                try capture(window, name: "character-state")
                window.contentView = NSHostingView(rootView: StateSheet(model: model, personaID: model.selectedID!, initialTab: "schema"))
                try await Task.sleep(for: .milliseconds(400))
                try capture(window, name: "response-schema")
                print("Rendered actual SwiftUI views at 1000×730 and 820×610; composer sizing, native marked text, Chinese commit, Enter send, Command–Enter/Shift–Enter newlines, empty draft, and input selection passed.")
                model.closeChat()
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
