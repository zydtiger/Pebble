import AppKit
import SwiftMath

@MainActor
final class FormulaAttachment: NSTextAttachment {
    let source: String

    init?(source: String, display: Bool) {
        let latex = String(source.dropFirst(2).dropLast(2))
        guard !latex.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        var renderer = MathImage(latex: latex, fontSize: display ? 20 : 14,
                                 textColor: NSColor(PebbleTheme.ink),
                                 labelMode: display ? .display : .text, textAlignment: .left)
        let (error, image, metrics) = renderer.asImage()
        guard error == nil, let image, let metrics,
              image.size.width.isFinite, image.size.height.isFinite,
              image.size.width > 0, image.size.height > 0 else { return nil }
        self.source = source
        super.init(data: nil, ofType: nil)
        image.accessibilityDescription = source
        attachmentCell = FormulaCell(image: image, descent: metrics.descent)
    }

    required init?(coder: NSCoder) { nil }
}

@MainActor
final class FormulaCell: NSTextAttachmentCell {
    let naturalSize: NSSize
    let descent: CGFloat
    private var scale: CGFloat = 1

    init(image: NSImage, descent: CGFloat) {
        naturalSize = image.size
        self.descent = descent
        super.init(imageCell: image)
    }

    required init(coder: NSCoder) { fatalError("Formula cells are created from response text") }

    func fit(to width: CGFloat) {
        scale = min(1, max(1, width) / naturalSize.width)
    }

    override func cellSize() -> NSSize {
        // AppKit's attachment metrics predate actor annotations; our TextKit layout stays on MainActor.
        MainActor.assumeIsolated {
            NSSize(width: naturalSize.width * scale, height: naturalSize.height * scale)
        }
    }

    override func cellBaselineOffset() -> NSPoint {
        MainActor.assumeIsolated { NSPoint(x: 0, y: -descent * scale) }
    }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?) {
        image?.draw(in: cellFrame, from: .zero, operation: .sourceOver, fraction: 1,
                    respectFlipped: true, hints: nil)
    }
}
