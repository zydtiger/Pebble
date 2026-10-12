import AppKit

@main
struct ImagePreviewChecks {
    @MainActor
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        Task { @MainActor in
            do {
                let root = URL(fileURLWithPath: ProcessInfo.processInfo.environment["PEBBLE_PREVIEW_DATA"]!)
                let source = root.appendingPathComponent("landscape.png")
                try picture().write(to: source)
                let window = ImagePreviewWindow.open(sourcePath: source.path, title: "Image preview")
                app.activate()
                try await pause()
                guard let scroll = window.contentView as? NSScrollView else { throw Failure.check("Missing image canvas") }
                try require(window.sheetParent == nil && window.styleMask.contains(.resizable), "Preview must be an independent resizable window")
                try require(scroll.allowsMagnification && scroll.documentView?.frame.size == NSSize(width: 1024, height: 640), "Original image and native pinch support missing")
                let initial = scroll.magnification
                let out = try button(window, id: "image-zoom-out")
                let zoom = try button(window, id: "image-zoom-in")
                let pin = try button(window, id: "image-pin")
                try require(out.convert(.zero, to: nil).x < zoom.convert(.zero, to: nil).x &&
                            zoom.convert(.zero, to: nil).x < pin.convert(.zero, to: nil).x, "Toolbar ordering changed")
                guard let controls = window.titlebarAccessoryViewControllers.first?.view.subviews.first as? NSStackView else {
                    throw Failure.check("Titlebar controls missing")
                }
                try require(controls.arrangedSubviews.contains(where: { $0 is NSBox }), "Toolbar separator missing")
                let centers = [out, zoom, pin].map { $0.convert(NSPoint(x: $0.bounds.midX, y: $0.bounds.midY), to: nil).y }
                try require(centers.max()! - centers.min()! < 1, "Titlebar controls are vertically misaligned")
                let hover = NSEvent.enterExitEvent(with: .mouseEntered, location: .zero, modifierFlags: [], timestamp: 0,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 0, trackingNumber: 0, userData: nil)!
                for control in [out, zoom, pin] {
                    let tint = control.contentTintColor
                    control.mouseEntered(with: hover)
                    try require(control.alphaValue == 1 && control.contentTintColor == tint, "Hover changed the icon appearance")
                    control.mouseExited(with: hover)
                    try require(control.alphaValue == 1 && control.contentTintColor == tint, "Hover exit changed the icon appearance")
                }
                try capture(window, name: "image-window-fit")
                zoom.mouseEntered(with: hover)
                try capture(window, name: "image-window-hover")
                zoom.mouseExited(with: hover)
                zoom.performClick(nil)
                try require(scroll.magnification > initial, "Zoom in did not change scale")
                out.performClick(nil)
                try require(abs(scroll.magnification - initial) < 0.001, "Zoom out did not restore scale")
                for _ in 0..<5 { zoom.performClick(nil) }
                let visible = scroll.documentVisibleRect
                try require(visible.width < 1024 && visible.height < 640, "Magnified content cannot be explored")
                scroll.contentView.scroll(to: NSPoint(x: 200, y: 150))
                scroll.reflectScrolledClipView(scroll.contentView)
                try require(scroll.documentVisibleRect.origin.x > 0, "Panning did not move within the image")
                try capture(window, name: "image-window-zoomed")
                let manualScale = scroll.magnification
                window.setContentSize(NSSize(width: 520, height: 380))
                try await pause()
                try require(abs(scroll.magnification - manualScale) < 0.001, "Resizing discarded manual zoom")
                pin.performClick(nil)
                try require(window.level == .floating && pin.state == .on, "Pin did not raise the window")
                try capture(window, name: "image-window-pinned")
                pin.performClick(nil)
                try require(window.level == .normal && pin.state == .off, "Unpin did not restore normal level")
                for _ in 0..<40 { zoom.performClick(nil) }
                try require(scroll.magnification == scroll.maxMagnification && !zoom.isEnabled, "Upper zoom limit failed")
                for _ in 0..<60 { out.performClick(nil) }
                try require(scroll.magnification == scroll.minMagnification && !out.isEnabled, "Lower zoom limit failed")
                let reopened = ImagePreviewWindow.open(sourcePath: source.path)
                try require(reopened === window, "Reopening the same picture duplicated its window")
                let escape = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                    windowNumber: window.windowNumber, context: nil, characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}", isARepeat: false, keyCode: 53)!
                window.sendEvent(escape)
                try await pause()
                try require(!window.isVisible, "Escape did not close the preview")
                let fresh = ImagePreviewWindow.open(sourcePath: source.path)
                try require(fresh !== window && fresh.level == .normal, "Closed preview controller was retained")
                fresh.close()
                let missing = ImagePreviewWindow.open(sourcePath: root.appendingPathComponent("missing.png").path)
                try await pause()
                try require(!(try button(missing, id: "image-zoom-in")).isEnabled, "Missing image still allows zoom")
                try capture(missing, name: "image-window-missing")
                missing.close()
                print("PASS: independent window, full image, zoom/pan/limits, resize, toolbar separator, pin/unpin, Escape and reopen lifecycle")
                app.terminate(nil)
            } catch {
                print("FAIL: \(error)")
                exit(1)
            }
        }
        app.run()
    }

    static func require(_ condition: Bool, _ message: String) throws {
        if !condition { throw Failure.check(message) }
    }
    static func pause() async throws { try await Task.sleep(for: .milliseconds(250)) }
    @MainActor
    static func button(_ window: NSWindow, id: String) throws -> NSButton {
        func find(_ view: NSView) -> NSButton? {
            if let button = view as? NSButton, button.accessibilityIdentifier() == id { return button }
            return view.subviews.lazy.compactMap(find).first
        }
        guard let controls = window.titlebarAccessoryViewControllers.first?.view, let button = find(controls) else {
            throw Failure.check("Missing toolbar button: \(id)")
        }
        return button
    }
    @MainActor
    static func picture() throws -> Data {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 1024, pixelsHigh: 640,
                                     bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                                     isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
        NSColor(calibratedRed: 0.94, green: 0.92, blue: 0.85, alpha: 1).setFill()
        NSRect(x: 0, y: 0, width: 1024, height: 640).fill()
        NSColor(calibratedRed: 0.35, green: 0.46, blue: 0.39, alpha: 1).setFill()
        NSBezierPath(ovalIn: NSRect(x: 90, y: 160, width: 550, height: 300)).fill()
        NSColor(calibratedRed: 0.77, green: 0.60, blue: 0.48, alpha: 1).setFill()
        NSBezierPath(ovalIn: NSRect(x: 570, y: 280, width: 340, height: 250)).fill()
        NSColor.gray.setStroke()
        NSBezierPath(rect: NSRect(x: 12, y: 12, width: 1000, height: 616)).stroke()
        NSGraphicsContext.restoreGraphicsState()
        guard let data = bitmap.representation(using: .png, properties: [:]) else { throw Failure.check("PNG encoding failed") }
        return data
    }
    @MainActor
    static func capture(_ window: NSWindow, name: String) throws {
        guard let view = window.contentView?.superview,
              let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { throw Failure.check("Missing window bitmap") }
        view.layoutSubtreeIfNeeded()
        view.cacheDisplay(in: view.bounds, to: bitmap)
        guard let png = bitmap.representation(using: .png, properties: [:]) else { throw Failure.check("Screenshot encoding failed") }
        try png.write(to: URL(fileURLWithPath: "build/validation/image-window/\(name).png"))
    }
    enum Failure: Error { case check(String) }
}
