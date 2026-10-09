import AppKit

extension RenderChecks {
    @MainActor
    static func pressReturn(_ editor: NSTextView, in window: NSWindow,
                            modifiers: NSEvent.ModifierFlags = []) throws {
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
                                          timestamp: 0, windowNumber: window.windowNumber, context: nil,
                                          characters: "\r", charactersIgnoringModifiers: "\r",
                                          isARepeat: false, keyCode: 36) else {
            throw RenderError.failed("Could not create Return key event")
        }
        window.makeFirstResponder(editor)
        NSApplication.shared.sendEvent(event)
    }

    @MainActor
    static func checkComposerSizing(window: NSWindow, model: ChatModel, editor: NSTextView) async throws {
        guard let scroll = editor.enclosingScrollView else { throw RenderError.failed("Missing composer viewport") }

        model.draft = ""
        try await Task.sleep(for: .milliseconds(100))
        let initialHeight = scroll.frame.height
        guard initialHeight <= 34, !scroll.hasVerticalScroller else {
            throw RenderError.failed("The empty composer should be compact without a scrollbar")
        }

        editor.insertText("One small thought.\nAnd another.\n", replacementRange: NSRange(location: 0, length: 0))
        try await Task.sleep(for: .milliseconds(100))
        guard scroll.frame.height > initialHeight + 20, scroll.frame.height < 160, !scroll.hasVerticalScroller else {
            throw RenderError.failed("New lines did not grow the composer before scrolling")
        }
        try capture(window, name: "composer-multiline")

        model.draft = String(repeating: "A small thought worth sharing. ", count: 12)
        try await Task.sleep(for: .milliseconds(100))
        guard scroll.frame.height > initialHeight, !scroll.hasVerticalScroller else {
            throw RenderError.failed("Wrapped text did not grow the composer")
        }
        let wideHeight = scroll.frame.height
        window.setContentSize(NSSize(width: 820, height: 610))
        try await Task.sleep(for: .milliseconds(100))
        guard scroll.frame.height > wideHeight else {
            throw RenderError.failed("Composer height did not follow the narrower text layout")
        }

        model.draft = (1...20).map { "A small thought, line \($0)." }.joined(separator: "\n")
        try await Task.sleep(for: .milliseconds(100))
        guard abs(scroll.frame.height - 160) < 1, scroll.hasVerticalScroller else {
            throw RenderError.failed("The composer must stop growing at 160 pt and allow scrolling")
        }
        editor.scrollRangeToVisible(NSRange(location: editor.string.utf16.count, length: 0))
        try await Task.sleep(for: .milliseconds(100))
        guard scroll.contentView.bounds.origin.y > 0 else { throw RenderError.failed("Overflow text did not scroll") }
        guard let root = window.contentView, root.bounds.contains(scroll.convert(scroll.bounds, to: root)) else {
            throw RenderError.failed("The expanded composer escaped the compact window")
        }
        try capture(window, name: "composer-maximum-compact")
        window.setContentSize(NSSize(width: 1000, height: 730))
        try await Task.sleep(for: .milliseconds(100))
        try capture(window, name: "composer-maximum")

        editor.setSelectedRange(NSRange(location: 0, length: editor.string.utf16.count))
        editor.deleteBackward(nil)
        try await Task.sleep(for: .milliseconds(100))
        guard model.draft.isEmpty, abs(scroll.frame.height - initialHeight) < 1,
              !scroll.hasVerticalScroller, scroll.contentView.bounds.origin.y == 0 else {
            throw RenderError.failed("Clearing text must shrink the composer and reset scrolling")
        }
    }
}
