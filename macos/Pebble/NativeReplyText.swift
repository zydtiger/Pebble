import AppKit
import SwiftUI

struct NativeReplyText: NSViewRepresentable {
    let source: String
    var equation = false

    func makeNSView(context: Context) -> ReplyTextView {
        let view = ReplyTextView(frame: .zero)
        view.isEditable = false
        view.isSelectable = true
        view.drawsBackground = false
        view.textContainerInset = .zero
        view.textContainer?.lineFragmentPadding = 0
        view.textContainer?.widthTracksTextView = false
        view.isVerticallyResizable = false
        view.isHorizontallyResizable = false
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return view
    }

    func updateNSView(_ view: ReplyTextView, context: Context) {
        guard view.source != source || view.equation != equation else { return }
        view.source = source
        view.equation = equation
        view.textStorage?.setAttributedString(Self.attributed(source, equation: equation))
        view.setAccessibilityLabel(source)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: ReplyTextView, context: Context) -> CGSize? {
        if proposal.width == 0 || proposal.height == 0 { return .zero }
        return nsView.measuredSize(width: proposal.width)
    }

    static func attributed(_ source: String, equation: Bool = false) -> NSAttributedString {
        if equation {
            if let attachment = FormulaAttachment(source: source, display: true) {
                return NSAttributedString(attachment: attachment)
            }
            return NSAttributedString(string: source, attributes: baseAttributes)
        }
        // Neutral placeholders let emphasis span across formulas without exposing LaTeX to Markdown.
        let prefix = "PebbleMath" + UUID().uuidString.replacingOccurrences(of: "-", with: "")
        var protected = ""
        var replacements: [(String, ReplyMathToken)] = []
        for token in ReplyMathToken.parse(source) {
            if case .text(let text) = token {
                protected += text
            } else {
                let key = prefix + "Token" + String(replacements.count) + "End"
                protected += key
                replacements.append((key, token))
            }
        }
        let styled = ReplyMarkdownBlock.inline(protected)
        let result = NSMutableAttributedString(string: "")
        for run in styled.runs {
            var attributes = baseAttributes
            let intent = run.inlinePresentationIntent ?? []
            var font = intent.contains(.code) ? NSFont.monospacedSystemFont(ofSize: 14, weight: .regular) : NSFont.systemFont(ofSize: 14)
            if intent.contains(.stronglyEmphasized) { font = NSFontManager.shared.convert(font, toHaveTrait: .boldFontMask) }
            if intent.contains(.emphasized) { font = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask) }
            attributes[.font] = font
            if let link = run.link { attributes[.link] = link }
            if intent.contains(.strikethrough) { attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
            result.append(NSAttributedString(string: String(styled[run.range].characters), attributes: attributes))
        }
        for (key, token) in replacements {
            let range = (result.string as NSString).range(of: key)
            guard range.location != NSNotFound else { continue }
            let attributes = result.attributes(at: range.location, effectiveRange: nil)
            let replacement: NSMutableAttributedString
            if case .formula(let raw, let display) = token,
               let attachment = FormulaAttachment(source: raw, display: display) {
                replacement = NSMutableAttributedString(attachment: attachment)
                replacement.addAttributes(attributes, range: NSRange(location: 0, length: replacement.length))
            } else {
                replacement = NSMutableAttributedString(string: token.source, attributes: attributes)
            }
            result.replaceCharacters(in: range, with: replacement)
        }
        return result
    }

    private static var baseAttributes: [NSAttributedString.Key: Any] {
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = 6
        return [.font: NSFont.systemFont(ofSize: 14), .foregroundColor: NSColor(PebbleTheme.ink), .paragraphStyle: paragraph]
    }
}

final class ReplyTextView: NSTextView {
    var source: String?
    var equation = false

    override var intrinsicContentSize: NSSize {
        NSSize(width: NSView.noIntrinsicMetric, height: NSView.noIntrinsicMetric)
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        configureLayout(width: newSize.width)
    }

    private func configureLayout(width: CGFloat) {
        guard let storage = textStorage, let container = textContainer, let manager = layoutManager else { return }
        let range = NSRange(location: 0, length: storage.length)
        storage.enumerateAttribute(.attachment, in: range) { value, _, _ in
            ((value as? FormulaAttachment)?.attachmentCell as? FormulaCell)?.fit(to: max(1, width))
        }
        manager.invalidateLayout(forCharacterRange: range, actualCharacterRange: nil)
        container.containerSize = NSSize(width: max(1, width), height: CGFloat.greatestFiniteMagnitude)
        manager.ensureLayout(for: container)
    }

    func measuredSize(width proposedWidth: CGFloat?) -> CGSize {
        guard let storage = textStorage else { return .zero }
        // Measure in a separate TextKit stack: SwiftUI's speculative proposals must not resize the live editor.
        let measuredStorage = NSTextStorage(attributedString: storage)
        let manager = NSLayoutManager()
        let width = proposedWidth ?? CGFloat.greatestFiniteMagnitude
        let container = NSTextContainer(size: NSSize(width: width, height: CGFloat.greatestFiniteMagnitude))
        container.lineFragmentPadding = 0
        manager.addTextContainer(container)
        measuredStorage.addLayoutManager(manager)
        measuredStorage.enumerateAttribute(.attachment, in: NSRange(location: 0, length: measuredStorage.length)) { value, _, _ in
            ((value as? FormulaAttachment)?.attachmentCell as? FormulaCell)?.fit(to: width)
        }
        manager.ensureLayout(for: container)
        let used = manager.usedRect(for: container)
        let size = NSSize(width: max(1, ceil(used.width)),
                          height: max(1, ceil(used.height + manager.extraLineFragmentRect.height)))
        configureLayout(width: bounds.width)
        return size
    }

    static func copyText(_ text: NSAttributedString) -> String {
        let result = NSMutableAttributedString(attributedString: text)
        result.enumerateAttribute(.attachment, in: NSRange(location: 0, length: result.length), options: .reverse) { value, range, _ in
            if let formula = value as? FormulaAttachment { result.replaceCharacters(in: range, with: formula.source) }
        }
        return result.string
    }

    override func copy(_ sender: Any?) {
        guard let storage = textStorage, selectedRange().length > 0 else { return }
        let selected = storage.attributedSubstring(from: selectedRange())
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(Self.copyText(selected), forType: .string)
    }
}
