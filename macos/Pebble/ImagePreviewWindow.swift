import AppKit
import SwiftUI

@MainActor
final class ImagePreviewWindow: NSWindowController, NSWindowDelegate {
    private static var previews: [String: ImagePreviewWindow] = [:]
    private let sourcePath: String
    private let scrollView = NSScrollView()
    private let imageView = NSImageView()
    private let zoomOut = PreviewToolbarButton()
    private let zoomIn = PreviewToolbarButton()
    private let pin = PreviewToolbarButton()
    private var fitsWindow = true

    @discardableResult
    static func open(sourcePath: String, title: String = "Image", unavailableMessage: String = "Image attachment is missing or corrupt.") -> NSWindow {
        let path = URL(fileURLWithPath: sourcePath).standardizedFileURL.path
        let preview = previews[path] ?? ImagePreviewWindow(sourcePath: path)
        previews[path] = preview
        preview.loadImage(title: title, unavailableMessage: unavailableMessage)
        preview.showWindow(nil)
        preview.window!.makeKeyAndOrderFront(nil)
        preview.fitImage()
        return preview.window!
    }

    private init(sourcePath: String) {
        self.sourcePath = sourcePath
        let window = PreviewWindow(contentRect: NSRect(x: 0, y: 0, width: 720, height: 520),
                                   styleMask: [.titled, .closable, .miniaturizable, .resizable],
                                   backing: .buffered, defer: false)
        super.init(window: window)
        window.identifier = NSUserInterfaceItemIdentifier("image-preview")
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.minSize = NSSize(width: 320, height: 240)
        window.appearance = NSAppearance(named: .aqua)
        window.backgroundColor = NSColor(PebbleTheme.canvas)
        window.delegate = self
        window.toolbarStyle = .unifiedCompact
        let toolbar = NSToolbar(identifier: "image-preview-toolbar")
        toolbar.displayMode = .iconOnly
        toolbar.allowsUserCustomization = false
        window.toolbar = toolbar
        configureTitlebar(window)

        scrollView.contentView = CenteredImageClipView()
        scrollView.backgroundColor = NSColor(PebbleTheme.canvas)
        scrollView.drawsBackground = true
        scrollView.hasHorizontalScroller = true
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.scrollerStyle = .overlay
        scrollView.allowsMagnification = true
        scrollView.maxMagnification = 8
        scrollView.minMagnification = 0.05
        scrollView.autoresizingMask = [.width, .height]
        imageView.imageScaling = .scaleNone
        scrollView.documentView = imageView
        NotificationCenter.default.addObserver(self, selector: #selector(beginMagnifying),
            name: NSScrollView.willStartLiveMagnifyNotification, object: scrollView)
        NotificationCenter.default.addObserver(self, selector: #selector(endMagnifying),
            name: NSScrollView.didEndLiveMagnifyNotification, object: scrollView)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    private func loadImage(title: String, unavailableMessage: String) {
        guard let window else { return }
        window.title = title
        fitsWindow = true
        guard let image = NSImage(contentsOfFile: sourcePath), image.size.width > 0, image.size.height > 0 else {
            imageView.image = nil
            window.contentView = NSHostingView(rootView: Text(unavailableMessage)
                .foregroundStyle(PebbleTheme.muted).padding(30)
                .frame(maxWidth: .infinity, maxHeight: .infinity))
            updateZoomButtons()
            if !window.isVisible { window.center() }
            return
        }
        imageView.image = image
        imageView.setFrameSize(image.size)
        imageView.setAccessibilityLabel(title)
        window.contentView = scrollView
        if !window.isVisible {
            let screen = NSApp.keyWindow?.screen ?? NSScreen.main
            let available = screen?.visibleFrame.size ?? NSSize(width: 1200, height: 900)
            let scale = min(1, min(1000, available.width * 0.85) / image.size.width,
                            min(760, available.height * 0.8) / image.size.height)
            window.setContentSize(NSSize(width: max(420, image.size.width * scale),
                                         height: max(300, image.size.height * scale)))
            window.center()
        }
    }

    private func fitImage() {
        guard let image = imageView.image else { return }
        scrollView.layoutSubtreeIfNeeded()
        let scale = min(1, scrollView.contentSize.width / image.size.width,
                        scrollView.contentSize.height / image.size.height)
        guard scale > 0 else { return }
        scrollView.minMagnification = min(0.05, scale / 4)
        scrollView.setMagnification(scale, centeredAt: NSPoint(x: image.size.width / 2, y: image.size.height / 2))
        updateZoomButtons()
    }

    private func configureTitlebar(_ window: NSWindow) {
        configure(zoomOut, symbol: "minus.magnifyingglass", label: "Zoom out", id: "image-zoom-out", action: #selector(decreaseZoom))
        configure(zoomIn, symbol: "plus.magnifyingglass", label: "Zoom in", id: "image-zoom-in", action: #selector(increaseZoom))
        configure(pin, symbol: "pin", label: "Keep on top", id: "image-pin", action: #selector(togglePin))
        pin.setButtonType(.toggle)
        let separator = NSBox()
        separator.boxType = .separator
        separator.widthAnchor.constraint(equalToConstant: 1).isActive = true
        separator.heightAnchor.constraint(equalToConstant: 16).isActive = true
        let controls = NSStackView(views: [zoomOut, zoomIn, separator, pin])
        controls.orientation = .horizontal
        controls.alignment = .centerY
        controls.spacing = 6
        controls.translatesAutoresizingMaskIntoConstraints = false
        // A titlebar accessory keeps these plain controls out of toolbar glass grouping.
        let accessory = NSTitlebarAccessoryViewController()
        accessory.layoutAttribute = .right
        accessory.view = NSView(frame: NSRect(x: 0, y: 0, width: 129, height: 40))
        accessory.view.addSubview(controls)
        NSLayoutConstraint.activate([
            controls.leadingAnchor.constraint(equalTo: accessory.view.leadingAnchor, constant: 6),
            controls.trailingAnchor.constraint(equalTo: accessory.view.trailingAnchor, constant: -14),
            controls.centerYAnchor.constraint(equalTo: accessory.view.centerYAnchor),
        ])
        window.addTitlebarAccessoryViewController(accessory)
    }

    private func configure(_ button: NSButton, symbol: String, label: String, id: String, action: Selector) {
        button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        button.imagePosition = .imageOnly
        button.imageScaling = .scaleProportionallyDown
        button.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 14, weight: .regular)
        button.bezelStyle = .regularSquare
        button.isBordered = false
        button.contentTintColor = .secondaryLabelColor
        button.toolTip = label
        button.setAccessibilityLabel(label)
        button.setAccessibilityIdentifier(id)
        button.target = self
        button.action = action
        button.widthAnchor.constraint(equalToConstant: 30).isActive = true
        button.heightAnchor.constraint(equalToConstant: 30).isActive = true
    }

    @objc private func decreaseZoom() { zoom(by: 1 / 1.25) }
    @objc private func increaseZoom() { zoom(by: 1.25) }
    private func zoom(by factor: CGFloat) {
        guard imageView.image != nil else { return }
        fitsWindow = false
        let visible = scrollView.contentView.bounds
        let value = min(scrollView.maxMagnification, max(scrollView.minMagnification, scrollView.magnification * factor))
        scrollView.setMagnification(value, centeredAt: NSPoint(x: visible.midX, y: visible.midY))
        updateZoomButtons()
    }
    @objc private func beginMagnifying() { fitsWindow = false }
    @objc private func endMagnifying() { updateZoomButtons() }
    private func updateZoomButtons() {
        zoomOut.isEnabled = imageView.image != nil && scrollView.magnification > scrollView.minMagnification
        zoomIn.isEnabled = imageView.image != nil && scrollView.magnification < scrollView.maxMagnification
    }
    @objc private func togglePin() {
        let pinned = pin.state == .on
        window?.level = pinned ? .floating : .normal
        pin.image = NSImage(systemSymbolName: pinned ? "pin.fill" : "pin", accessibilityDescription: "Keep on top")
        pin.contentTintColor = pinned ? NSColor(PebbleTheme.sage) : .secondaryLabelColor
        pin.toolTip = pinned ? "Stop keeping on top" : "Keep on top"
    }
    func windowDidResize(_ notification: Notification) {
        if fitsWindow { fitImage() }
    }
    func windowWillClose(_ notification: Notification) {
        NotificationCenter.default.removeObserver(self)
        Self.previews.removeValue(forKey: sourcePath)
    }
}

private final class PreviewToolbarButton: NSButton {
    private var hoverArea: NSTrackingArea?
    private var hovered = false { didSet { needsDisplay = true } }
    private var pressed = false { didSet { needsDisplay = true } }

    override var isEnabled: Bool { didSet { needsDisplay = true } }

    override func draw(_ dirtyRect: NSRect) {
        if isEnabled && (hovered || pressed) {
            NSColor.labelColor.withAlphaComponent(pressed ? 0.12 : 0.08).setFill()
            NSBezierPath(roundedRect: bounds, xRadius: 6, yRadius: 6).fill()
        }
        super.draw(dirtyRect)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let hoverArea { removeTrackingArea(hoverArea) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                  owner: self, userInfo: nil)
        addTrackingArea(area)
        hoverArea = area
        hovered = window.map { bounds.contains(convert($0.mouseLocationOutsideOfEventStream, from: nil)) } ?? false
    }

    override func mouseEntered(with event: NSEvent) { hovered = true }
    override func mouseExited(with event: NSEvent) { hovered = false }
    override func mouseDown(with event: NSEvent) {
        pressed = true
        defer { pressed = false }
        super.mouseDown(with: event)
    }
}

private final class PreviewWindow: NSWindow {
    override func cancelOperation(_ sender: Any?) { performClose(sender) }
}

private final class CenteredImageClipView: NSClipView {
    override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
        var bounds = super.constrainBoundsRect(proposedBounds)
        guard let documentView else { return bounds }
        if bounds.width > documentView.frame.width {
            bounds.origin.x = (documentView.frame.width - bounds.width) / 2
        }
        if bounds.height > documentView.frame.height {
            bounds.origin.y = (documentView.frame.height - bounds.height) / 2
        }
        return bounds
    }
}
