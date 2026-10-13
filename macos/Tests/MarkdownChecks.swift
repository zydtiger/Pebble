import AppKit
import SwiftUI

@main
struct MarkdownChecks {
    static func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
        if !condition() { throw Failure.check(message) }
    }

    static func checkParsing() throws {
        let inline = ReplyMarkdownBlock.inline("**Bold** and *italic*, ***both***.\n\n中文 _斜体_ and \\*literal\\*.")
        try require(String(inline.characters) == "Bold and italic, both.\n\n中文 斜体 and *literal*.", "Inline text or line breaks changed")
        for (word, intents) in [("Bold", InlinePresentationIntent.stronglyEmphasized),
                                ("italic", .emphasized), ("both", [.emphasized, .stronglyEmphasized]),
                                ("斜体", .emphasized)] {
            guard let range = inline.range(of: word) else { throw Failure.check("Missing styled word") }
            try require(inline[range].runs.allSatisfy { $0.inlinePresentationIntent?.isSuperset(of: intents) == true }, "Missing emphasis for \(word)")
        }
        try require(String(ReplyMarkdownBlock.inline("An unmatched **marker").characters) == "An unmatched **marker", "Unmatched emphasis lost text")
        let cases: [(String, [ReplyMarkdownBlock])] = [
            ("Plain\n\ntext", [.prose("Plain\n\ntext")]),
            ("Before **bold**\n```swift\n  let x = \"*literal*\"\n\n  print(x)\n```\nAfter *italic*",
             [.prose("Before **bold**"), .code("  let x = \"*literal*\"\n\n  print(x)"), .prose("After *italic*")]),
            ("```\na\n```\n~~~text\nb\n~~~", [.code("a"), .code("b")]),
            ("````\na\n```\nb\n`````", [.code("a\n```\nb")]),
            ("~~~\na\n```\nb\n~~~", [.code("a\n```\nb")]),
            ("```\na\n``` not a closer\nb\n```", [.code("a\n``` not a closer\nb")]),
            ("  ```swift\n    indented\n x\n  ```  \t", [.code("  indented\nx")]),
            ("    ```\ntext", [.prose("    ```\ntext")]),
            ("Text with ```inline```", [.prose("Text with ```inline```")]),
            ("```bad`info\ntext", [.prose("```bad`info\ntext")]),
            ("Before\n```swift\n  unfinished\n", [.prose("Before"), .code("  unfinished\n")]),
            ("```\n```", [.code("")]),
            ("before\r\n```\r\n  code\r\n```\r\nafter", [.prose("before"), .code("  code"), .prose("after")])
        ]
        for (source, expected) in cases {
            try require(ReplyMarkdownBlock.parse(source) == expected, "Fence parsing mismatch: \(source.debugDescription)")
        }
        print("Markdown parsing passed: emphasis, nesting, Unicode, escapes, whitespace, fenced and unfinished code.")
    }

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
                try checkParsing()
                try checkMath()
                for _ in 0..<200 where model.loading { try await Task.sleep(for: .milliseconds(20)) }
                try require(model.selected != nil && model.error == nil, "Demo workspace did not load")
                let fixture = #"""
                **Bold** and *italic*: \(x^2\).
                ```swift
                  let text = "**literal**"
                ```
                \[\frac{1}{2}\]
                After.
                """#
                model.sendMessage(fixture)
                for _ in 0..<300 where model.isReplying { try await Task.sleep(for: .milliseconds(20)) }
                try require(!model.isReplying && model.error == nil, "Demo reply did not finish")
                try require(model.bubbles.contains { $0.isUser && $0.text == fixture }, "User source changed")
                guard let reply = model.bubbles.last(where: { !$0.isUser }) else { throw Failure.check("No reply") }
                try require(reply.text.contains(fixture), "Response Markdown source was altered")
                try await Task.sleep(for: .milliseconds(200))
                try capture(window, "reply-wide")
                window.setContentSize(NSSize(width: 820, height: 610))
                try await Task.sleep(for: .milliseconds(200))
                try capture(window, "reply-compact")
                await model.load()
                try require(model.bubbles.last(where: { !$0.isUser })?.text == reply.text, "Reload changed stored Markdown")

                let longCode = "**Long code stays inside the bubble.**\n```\n    " + String(repeating: "plain_code ", count: 30) + "\n\n    *literal*\n```\n*Text after code remains visible.*"
                let host = NSHostingView(rootView: ReplyMarkdown(text: longCode)
                    .font(.system(size: 14)).lineSpacing(6).textSelection(.enabled)
                    .foregroundStyle(PebbleTheme.ink).padding(18)
                    .frame(width: 340).background(PebbleTheme.canvas))
                let codeWindow = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 340, height: 320),
                                          styleMask: [.titled], backing: .buffered, defer: false)
                codeWindow.contentView = host
                codeWindow.makeKeyAndOrderFront(nil)
                try await Task.sleep(for: .milliseconds(200))
                try capture(codeWindow, "long-code")
                try require(host.fittingSize.height > 100 && host.fittingSize.height < 320, "Code layout collapsed or expanded vertically: \(host.fittingSize)")
                print("Native Markdown render passed: private reply, plain user message, compact window, stored source reload, long code.")
                codeWindow.orderOut(nil)
                try await renderMath()
                model.closeChat()
                app.terminate(nil)
            } catch {
                FileHandle.standardError.write(Data("Markdown check failed: \(error)\n".utf8))
                exit(1)
            }
        }
        app.run()
    }

    @MainActor
    static func capture(_ window: NSWindow, _ name: String) throws {
        guard let view = window.contentView,
              let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { throw Failure.check("Missing window bitmap") }
        view.layoutSubtreeIfNeeded()
        view.cacheDisplay(in: view.bounds, to: bitmap)
        guard let png = bitmap.representation(using: .png, properties: [:]) else { throw Failure.check("Missing PNG") }
        try png.write(to: URL(fileURLWithPath: "build/validation/reply-markdown/\(name).png"))
    }

    enum Failure: Error { case check(String) }
}
