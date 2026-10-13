import AppKit
import SwiftUI

extension MarkdownChecks {
    @MainActor
    static func checkMath() throws {
        let inline = #"A \(x^2\) and \(\frac{1}{2}\)."#
        try require(ReplyMathToken.parse(inline) == [.text("A "), .formula(#"\(x^2\)"#, display: false), .text(" and "), .formula(#"\(\frac{1}{2}\)"#, display: false), .text(".")], "Inline delimiters were not recognized")
        let display = "Before\n\\[\n\\int_0^1 x^2 dx = \\frac{1}{3}\n\\]\nAfter"
        try require(ReplyMarkdownBlock.parse(display) == [.prose("Before"), .equation("\\[\n\\int_0^1 x^2 dx = \\frac{1}{3}\n\\]"), .prose("After")], "Multiline equation boundaries changed")
        let escaped = #"Literal \\(x\\), `\(y\)` and ``\[z\]``."#
        try require(ReplyMathToken.parse(escaped) == [.text(escaped)], "Escaped delimiters or code spans became math")
        try require(ReplyMarkdownBlock.parse("```latex\n\\(x\\)\n\\[y\\]\n```") == [.code("\\(x\\)\n\\[y\\]")], "Fenced code parsed as math")
        for source in [#"\(\unknowncommand{x}\)"#, #"\[\frac{1}{\]"#, #"\(unfinished_x"#, #"\[unfinished **raw**"#] {
            let text = NativeReplyText.attributed(source)
            try require(text.string == source, "Invalid or unfinished formula lost its original text: \(source)")
            try require(attachments(in: text).isEmpty, "Invalid formula produced an attachment")
        }
        let rich = NativeReplyText.attributed(#"A **bold \(\frac{1}{2}\) tail** and *italic*."#)
        let formulas = attachments(in: rich)
        try require(formulas.count == 1, "Inline formula did not become a text attachment")
        let tail = (rich.string as NSString).range(of: "tail")
        let italic = (rich.string as NSString).range(of: "italic")
        let tailFont = rich.attribute(.font, at: tail.location, effectiveRange: nil) as! NSFont
        let italicFont = rich.attribute(.font, at: italic.location, effectiveRange: nil) as! NSFont
        try require(NSFontManager.shared.traits(of: tailFont).contains(.boldFontMask), "Bold emphasis did not span an inline formula")
        try require(NSFontManager.shared.traits(of: italicFont).contains(.italicFontMask), "Italic styling was lost")
        try require(ReplyTextView.copyText(rich) == #"A bold \(\frac{1}{2}\) tail and italic."#, "Copy did not retain inline LaTeX source")
        let cell = formulas[0].attachmentCell as! FormulaCell
        try require(cell.cellBaselineOffset().y < 0 && cell.cellSize().height > 10, "Fraction baseline or height is missing")
        let original = cell.cellSize()
        cell.fit(to: original.width / 2)
        try require(abs(cell.cellSize().width - original.width / 2) < 0.01, "Oversized inline formula cannot fit a line")
        cell.fit(to: .greatestFiniteMagnitude)
        try require(cell.cellSize() == original, "Inline formula failed to recover its natural size")
        for formula in [#"\[\int_0^1 x^2 dx = \frac{1}{3}\]"#, #"\[\begin{pmatrix}a & b \\ c & d\end{pmatrix}\]"#] {
            let rendered = NativeReplyText.attributed(formula, equation: true)
            try require(attachments(in: rendered).count == 1, "Display formula failed to render")
            try require(ReplyTextView.copyText(rendered) == formula, "Display formula copy changed source")
        }
        print("Math checks passed: delimiters, code exclusion, invalid-source fallback, emphasis, attachments, baseline metrics and copy.")
    }

    @MainActor
    static func attachments(in text: NSAttributedString) -> [FormulaAttachment] {
        var result: [FormulaAttachment] = []
        text.enumerateAttribute(.attachment, in: NSRange(location: 0, length: text.length)) { value, _, _ in
            if let attachment = value as? FormulaAttachment { result.append(attachment) }
        }
        return result
    }

    @MainActor
    static func renderMath() async throws {
        let source = #"""
        **Native math**: the fraction \(\frac{a+b}{c}\) stays in the same paragraph, with *italic text* and the identity \(e^{i\pi}+1=0\). More words make this paragraph wrap naturally when the window becomes narrow.
        \[\int_0^1 x^2\,dx = \frac{1}{3}\]
        A matrix:
        \[\begin{pmatrix}a & b \\ c & d\end{pmatrix}\]
        Invalid: \(\unknowncommand{x}\). Unfinished: \(x_1 +
        """#
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 580, height: 540),
                              styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.contentView = NSHostingView(rootView: ReplyMarkdown(text: source)
            .font(.system(size: 14)).textSelection(.enabled).padding(18)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(PebbleTheme.canvas))
        window.makeKeyAndOrderFront(nil)
        try await Task.sleep(for: .milliseconds(200))
        try require(abs(window.contentView!.bounds.height - 540) < 1, "Math content forced the window above its requested height")
        try capture(window, "math-wide")
        let wideHeight = paragraphViews(window.contentView!).first!.frame.height
        window.setContentSize(NSSize(width: 320, height: 640))
        try await Task.sleep(for: .milliseconds(200))
        try require(abs(window.contentView!.bounds.height - 640) < 1, "Narrow math content forced an oversized window")
        let paragraphs = paragraphViews(window.contentView!)
        try require(paragraphs.first!.frame.height > wideHeight, "Inline prose did not wrap at narrower width")
        for paragraph in paragraphs {
            try require(paragraph.frame.height > 10, "Formula text view collapsed")
            try require(paragraph.layoutManager!.usedRect(for: paragraph.textContainer!).width <= paragraph.bounds.width + 1, "Paragraph is clipped instead of wrapping")
            try require(paragraph.layoutManager!.usedRect(for: paragraph.textContainer!).maxY <= paragraph.bounds.height + 1, "TextKit content exceeds its reported height")
        }
        try capture(window, "math-narrow")
        let long = #"\["# + (1...40).map { "x_{\($0)}" }.joined(separator: " + ") + #"\]"#
        window.contentView = NSHostingView(rootView: ReplyMarkdown(text: long)
            .padding(18).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(PebbleTheme.canvas))
        try await Task.sleep(for: .milliseconds(200))
        try require(paragraphViews(window.contentView!).first!.frame.width > 320, "Display equation was shrunk instead of horizontally scrollable")
        try capture(window, "math-long")
        guard let scroll = paragraphViews(window.contentView!).first?.enclosingScrollView else {
            throw Failure.check("Wide equation has no horizontal scroll view")
        }
        scroll.contentView.scroll(to: NSPoint(x: 200, y: 0))
        scroll.reflectScrolledClipView(scroll.contentView)
        try await Task.sleep(for: .milliseconds(100))
        try require(scroll.contentView.bounds.minX > 0, "Wide equation did not scroll horizontally")
        try capture(window, "math-long-scrolled")
        window.orderOut(nil)
        print("Native math render passed: inline wrapping, display integral/matrix, fallback text, compact sizing and wide equation.")
    }

    @MainActor
    static func paragraphViews(_ view: NSView) -> [ReplyTextView] {
        if let text = view as? ReplyTextView { return [text] }
        return view.subviews.flatMap { paragraphViews($0) }
    }
}
