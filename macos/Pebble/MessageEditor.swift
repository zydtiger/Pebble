import AppKit
import SwiftUI

struct MessageEditor: NSViewRepresentable {
    @Binding var text: String
    @Binding var height: CGFloat
    @Binding var isFocused: Bool
    var isEditable: Bool
    var onSubmit: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> ComposerScrollView {
        let scroll = ComposerScrollView()
        let coordinator = context.coordinator
        coordinator.scroll = scroll
        scroll.editor.delegate = coordinator
        scroll.onHeightChange = { [weak coordinator] height in
            DispatchQueue.main.async {
                guard let coordinator, coordinator.parent.height != height else { return }
                coordinator.parent.height = height
            }
        }
        scroll.editor.onFocusChange = { [weak coordinator] focused in
            guard let coordinator, coordinator.parent.isFocused != focused else { return }
            coordinator.parent.isFocused = focused
        }
        scroll.editor.onSubmit = { [weak coordinator] in coordinator?.parent.onSubmit() }
        scroll.onAttach = { [weak coordinator] in coordinator?.focusIfNeeded() }
        return scroll
    }

    func updateNSView(_ scroll: ComposerScrollView, context: Context) {
        context.coordinator.parent = self
        let editor = scroll.editor
        // A binding refresh must not replace the input method's marked text.
        if editor.string != text && !editor.hasMarkedText() {
            editor.string = text
            editor.setSelectedRange(NSRange(location: text.utf16.count, length: 0))
        }
        editor.isEditable = isEditable
        editor.needsDisplay = true
        scroll.fitText()
        context.coordinator.focusIfNeeded()
    }

    static func dismantleNSView(_ scroll: ComposerScrollView, coordinator: Coordinator) {
        scroll.editor.delegate = nil
        scroll.editor.onFocusChange = nil
        scroll.editor.onSubmit = nil
        scroll.onAttach = nil
        scroll.onHeightChange = nil
    }

    @MainActor
    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: MessageEditor
        weak var scroll: ComposerScrollView?

        init(_ parent: MessageEditor) { self.parent = parent }

        func textDidChange(_ notification: Notification) {
            guard let scroll else { return }
            parent.text = scroll.editor.string
            scroll.editor.needsDisplay = true
            scroll.fitText()
        }

        func focusIfNeeded() {
            guard parent.isFocused, parent.isEditable, let scroll, let window = scroll.window,
                  window.firstResponder !== scroll.editor else { return }
            window.makeFirstResponder(scroll.editor)
        }
    }
}

final class ComposerScrollView: NSScrollView {
    static let minimumHeight: CGFloat = 33
    static let maximumHeight: CGFloat = 160
    let editor: ComposerTextView
    var onHeightChange: ((CGFloat) -> Void)?
    var onAttach: (() -> Void)?
    private var fitting = false
    private var reportedHeight: CGFloat?

    init() {
        let storage = NSTextStorage()
        let layout = NSLayoutManager()
        let container = NSTextContainer(containerSize: NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude))
        storage.addLayoutManager(layout)
        layout.addTextContainer(container)
        editor = ComposerTextView(frame: .zero, textContainer: container)
        super.init(frame: .zero)

        drawsBackground = false
        borderType = .noBorder
        hasHorizontalScroller = false
        hasVerticalScroller = false
        autohidesScrollers = true
        scrollerStyle = .overlay
        documentView = editor

        editor.isRichText = false
        editor.importsGraphics = false
        editor.allowsUndo = true
        editor.drawsBackground = false
        editor.font = .systemFont(ofSize: 13)
        editor.textColor = NSColor(PebbleTheme.ink)
        editor.insertionPointColor = NSColor(PebbleTheme.ink)
        editor.textContainerInset = NSSize(width: 5, height: 8)
        editor.isVerticallyResizable = true
        editor.isHorizontallyResizable = false
        editor.autoresizingMask = [.width]
        editor.minSize = NSSize(width: 0, height: Self.minimumHeight)
        editor.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        container.widthTracksTextView = true
        container.lineFragmentPadding = 0
        editor.setAccessibilityLabel("Message")
        editor.setAccessibilityIdentifier("message-composer")
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        DispatchQueue.main.async { [weak self] in self?.onAttach?() }
    }

    override func layout() {
        super.layout()
        fitText()
    }

    func fitText() {
        guard !fitting, contentSize.width > 0,
              let container = editor.textContainer, let layout = editor.layoutManager,
              let font = editor.font else { return }
        fitting = true
        defer { fitting = false }

        editor.setFrameSize(NSSize(width: contentSize.width, height: editor.frame.height))
        layout.ensureLayout(for: container)
        // Include the extra line fragment so a trailing Return also grows the editor.
        let textHeight = max(layout.usedRect(for: container).maxY, layout.extraLineFragmentRect.maxY,
                             layout.defaultLineHeight(for: font))
        let documentHeight = ceil(textHeight + editor.textContainerInset.height * 2)
        let viewportHeight = min(Self.maximumHeight, max(Self.minimumHeight, documentHeight))
        let startsOverflowing = !hasVerticalScroller && documentHeight > Self.maximumHeight
        hasVerticalScroller = documentHeight > Self.maximumHeight
        editor.setFrameSize(NSSize(width: contentSize.width, height: max(viewportHeight, documentHeight)))
        if !hasVerticalScroller {
            contentView.scroll(to: .zero)
            reflectScrolledClipView(contentView)
        }
        if reportedHeight != viewportHeight {
            reportedHeight = viewportHeight
            onHeightChange?(viewportHeight)
        }
        if startsOverflowing { flashScrollers() }
    }
}

final class ComposerTextView: NSTextView {
    var onFocusChange: ((Bool) -> Void)?
    var onSubmit: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        guard (event.keyCode == 36 || event.keyCode == 76), isEditable, !hasMarkedText() else {
            super.keyDown(with: event)
            return
        }
        let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
        if modifiers.isEmpty {
            onSubmit?()
        } else if modifiers.subtracting([.command, .shift]).isEmpty {
            insertNewline(nil)
        } else {
            super.keyDown(with: event)
        }
    }

    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        if accepted { onFocusChange?(true) }
        return accepted
    }

    override func resignFirstResponder() -> Bool {
        let accepted = super.resignFirstResponder()
        if accepted { onFocusChange?(false) }
        return accepted
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard string.isEmpty, !hasMarkedText(), let font else { return }
        let placeholder = NSAttributedString(string: "Leave a little thought…", attributes: [
            .font: font,
            .foregroundColor: NSColor(PebbleTheme.muted).withAlphaComponent(0.8)
        ])
        placeholder.draw(at: NSPoint(x: textContainerOrigin.x + (textContainer?.lineFragmentPadding ?? 0),
                                     y: textContainerOrigin.y))
    }
}
