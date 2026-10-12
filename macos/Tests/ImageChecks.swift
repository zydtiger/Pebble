import AppKit
import SwiftUI

@MainActor
final class ImageDropFixture: NSObject, NSDraggingInfo {
    let draggingPasteboard: NSPasteboard
    let draggingDestinationWindow: NSWindow?
    init(_ board: NSPasteboard, window: NSWindow) { draggingPasteboard = board; draggingDestinationWindow = window }
    var draggingSourceOperationMask: NSDragOperation { .copy }
    var draggingLocation: NSPoint { .zero }
    var draggedImageLocation: NSPoint { .zero }
    nonisolated var draggedImage: NSImage? { nil }
    var draggingSource: Any? { nil }
    var draggingSequenceNumber: Int { 1 }
    var draggingFormation: NSDraggingFormation = .none
    var animatesToDestination = false
    var numberOfValidItemsForDrop = 1
    var springLoadingHighlight: NSSpringLoadingHighlight { .none }
    func slideDraggedImage(to screenPoint: NSPoint) { }
    nonisolated override func namesOfPromisedFilesDropped(atDestination dropDestination: URL) -> [String]? { nil }
    func resetSpringLoading() { }
    func enumerateDraggingItems(options: NSDraggingItemEnumerationOptions, for view: NSView?, classes classArray: [AnyClass], searchOptions: [NSPasteboard.ReadingOptionKey: Any], using block: (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void) { }
}

@main
struct ImageChecks {
    enum Failure: Error { case check(String) }
    @MainActor static func require(_ value: @autoclosure () -> Bool, _ detail: String) throws {
        if !value() { throw Failure.check(detail) }
    }
    @MainActor static func wait(_ condition: () -> Bool) async throws {
        for _ in 0..<600 { if condition() { return }; try await Task.sleep(for: .milliseconds(10)) }
        throw Failure.check("Native image lifecycle timed out")
    }
    @MainActor static func editor(_ view: NSView) -> ComposerTextView? {
        if let text = view as? ComposerTextView { return text }
        return view.subviews.lazy.compactMap { editor($0) }.first
    }
    @MainActor static func capture(_ window: NSWindow, _ name: String) throws {
        let view = window.contentView!
        view.layoutSubtreeIfNeeded()
        guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { throw Failure.check("Window bitmap unavailable") }
        view.cacheDisplay(in: view.bounds, to: bitmap)
        let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["PEBBLE_IMAGE_EVIDENCE"]!)
        try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("\(name).png"))
    }
    @MainActor static func accessible(_ element: Any, id: String, depth: Int = 0) -> NSObject? {
        guard depth < 40, let object = element as? NSObject else { return nil }
        let identifier = NSSelectorFromString("accessibilityIdentifier")
        if object.responds(to: identifier), object.perform(identifier)?.takeUnretainedValue() as? String == id { return object }
        let children = NSSelectorFromString("accessibilityChildren")
        if object.responds(to: children), let items = object.perform(children)?.takeUnretainedValue() as? [Any] {
            for item in items { if let found = accessible(item, id: id, depth: depth + 1) { return found } }
        }
        if let view = object as? NSView {
            for child in view.subviews { if let found = accessible(child, id: id, depth: depth + 1) { return found } }
        }
        return nil
    }
    @MainActor static func main() {
        let app = NSApplication.shared; app.setActivationPolicy(.accessory)
        let root = URL(fileURLWithPath: ProcessInfo.processInfo.environment["PEBBLE_DATA_DIR"]!)
        let model = ChatModel(dataRoot: root)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 730), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.contentView = NSHostingView(rootView: ContentView(model: model)); window.center(); window.makeKeyAndOrderFront(nil)
        Task { @MainActor in
            do {
                try await wait { model.selected != nil && !model.loading }
                var classifier = try await model.readClassifierSettings(nil)
                classifier.connection.enabled = false
                try await model.saveClassifierSettings(nil, update: classifier, credential: .keep)
                let firstPersona = model.selected!.id; let firstRoom = model.selectedID!
                let picture = NSImage(size: NSSize(width: 160, height: 100))
                picture.lockFocus(); NSColor.white.setFill(); NSRect(x: 0, y: 0, width: 160, height: 100).fill()
                NSColor.red.setFill(); NSBezierPath(ovalIn: NSRect(x: 30, y: 10, width: 80, height: 80)).fill(); picture.unlockFocus()
                let tiff = picture.tiffRepresentation!
                let bitmap = NSBitmapImageRep(data: tiff)!
                let source = root.appendingPathComponent("synthetic-circle.png")
                try bitmap.representation(using: .png, properties: [:])!.write(to: source)

                // Drive the real native picker in its modal run loop.
                let panel = NSOpenPanel()
                panel.directoryURL = root
                panel.nameFieldStringValue = source.lastPathComponent
                print("Select native image fixture: \(source.path)")
                fflush(stdout)
                model.pickImages(panel: panel)
                try await wait { !model.importingImages }
                try require(model.draftImages.count == 1, "Native picker did not import its selected PNG: \(model.error ?? "")")
                try await Task.sleep(for: .milliseconds(150)); try capture(window, "image-picker-draft")
                guard let text = editor(window.contentView!) else { throw Failure.check("Native composer unavailable") }
                let board = NSPasteboard.withUniqueName(); defer { board.releaseGlobally() }
                board.setData(tiff, forType: .tiff)
                try require(text.importImages(from: board), "Native pasteboard import did not handle TIFF")
                try await wait { !model.importingImages }
                try require(model.draftImages.count == 2, "TIFF paste did not normalize to PNG")
                board.clearContents(); board.writeObjects([source as NSURL])
                let drop = ImageDropFixture(board, window: window)
                try require(text.draggingEntered(drop) == .copy && text.performDragOperation(drop), "Native drop did not accept the file URL")
                try await wait { !model.importingImages }; try require(model.draftImages.count == 3, "Native drop did not import")
                model.removeImage(model.draftImages[1].id); try require(model.draftImages.count == 2, "Remove thumbnail failed")
                model.draft = "Images stay with this room."
                let retained = model.draftImages.map(\.id)
                model.importImages([.data(Data("invalid image".utf8))])
                try await wait { !model.importingImages }
                try require(model.error?.contains("Image attachment") == true && model.draftImages.map(\.id) == retained, "Import failure lost the existing draft")
                model.error = nil
                let created = await model.createCharacter("Second")
                try require(created, "Could not create draft-navigation fixture")
                try await wait { !model.loading }; model.select(firstRoom); try await wait { !model.loading }
                try require(model.draftImages.map(\.id) == retained && model.draft == "Images stay with this room.", "Room navigation lost image draft")
                model.mentionedIDs = ["invalid-private-mention"]
                for suggestion in ["Hello!", "A small thought for today."] {
                    model.sendMessage(suggestion); try await wait { !model.hasActiveReplies }
                    try require(model.error == nil, "Suggestion submitted draft mentions: \(model.error ?? "")")
                    try require(model.bubbles.filter(\.isUser).last?.text == suggestion && model.bubbles.filter(\.isUser).last?.images.isEmpty == true, "Suggestion submitted draft images or text")
                    try require(model.draft == "Images stay with this room." && model.draftImages.map(\.id) == retained && model.mentionedIDs == ["invalid-private-mention"], "Suggestion changed the existing text/image/mention draft")
                }
                try capture(window, "image-suggested-private-draft-preserved")
                model.sendMessage(); try await wait { !model.hasActiveReplies }
                try require(model.error != nil && model.draftImages.map(\.id) == retained, "Acceptance failure cleared image draft")
                model.mentionedIDs = []; model.error = nil; model.draft = ""
                model.sendMessage(); model.draft = "Edited while accepting."
                try await wait { !model.hasActiveReplies }
                try require(model.draftImages.map(\.id) == retained && model.draft == "Edited while accepting.", "Acceptance cleared a newer draft revision")
                try require(model.error == nil, "Image-only demo reply failed: \(model.error ?? "")")
                try require(model.bubbles.filter(\.isUser).last?.images.count == 2 && model.bubbles.filter(\.isUser).last?.text.isEmpty == true, "Image-only private message lost attachments")
                try capture(window, "image-only-private-and-newer-draft")
                model.draft = "Text and images."; model.sendMessage(); try await wait { !model.hasActiveReplies }
                try require(model.error == nil, "Text/image demo reply failed: \(model.error ?? "")")
                try require(model.draftImages.isEmpty && model.draft.isEmpty && model.bubbles.filter(\.isUser).last?.images.count == 2, "Exact accepted text/image draft was not cleared")
                try await Task.sleep(for: .milliseconds(150)); try capture(window, "image-transcript")
                if let image = model.bubbles.first(where: { !$0.images.isEmpty })?.images.first, let button = accessible(window.contentView!, id: "image-\(image.id)") {
                    let selector = NSSelectorFromString("accessibilityPerformPress")
                    typealias Press = @convention(c) (AnyObject, Selector) -> Bool
                    try require(unsafeBitCast(button.method(for: selector), to: Press.self)(button, selector), "Image open accessibility action failed")
                    try await wait { NSApp.windows.contains { $0.identifier?.rawValue == "image-preview" && $0.isVisible } }
                    guard let preview = NSApp.windows.first(where: { $0.identifier?.rawValue == "image-preview" && $0.isVisible }) else {
                        throw Failure.check("Image preview window unavailable")
                    }
                    try require(window.attachedSheet == nil, "Image preview blocked the conversation")
                    try capture(preview, "image-enlarged")
                    preview.performClose(nil)
                } else { throw Failure.check("Image attachment open control unavailable") }
                try await Task.sleep(for: .milliseconds(150))
                let grouped = await model.createGroup("Image group", members: [firstPersona])
                try require(grouped, "Could not create image group")
                try await wait { !model.loading }
                model.importImages([.file(source)]); try await wait { !model.importingImages }
                let groupImages = model.draftImages.map(\.id)
                model.draft = "Unsent group text with an image and mention."; model.mentionedIDs = [firstPersona]
                model.sendMessage("A separate group suggestion."); try await wait { !model.hasActiveReplies }
                try require(model.error == nil && model.bubbles.count == 1 && model.bubbles[0].text == "A separate group suggestion." && model.bubbles[0].images.isEmpty, "Group suggestion submitted draft images or forced draft mention replies")
                try require(model.draft == "Unsent group text with an image and mention." && model.draftImages.map(\.id) == groupImages && model.mentionedIDs == [firstPersona], "Group suggestion changed the draft text/images/mentions")
                try capture(window, "image-suggested-group-draft-preserved")
                model.draft = ""; model.mentionedIDs = []
                model.sendMessage(); try await wait { !model.hasActiveReplies }
                try require(model.bubbles.count == 2 && model.bubbles[1].images.count == 1 && model.bubbles[1].text.isEmpty, "Image-only public message did not retain its author/content")
                try await Task.sleep(for: .milliseconds(150)); try capture(window, "image-only-group")
                window.setContentSize(NSSize(width: 820, height: 610)); try await Task.sleep(for: .milliseconds(150)); try capture(window, "image-group-compact")
                model.closeChat()
                print("Native image checks passed: modal picker, TIFF pasteboard conversion, file drop, removal, room-local draft, import/accept failure, exact revision, isolated suggested sends preserving private/group drafts and mentions, private/group image-only sends, transcript and enlarged display.")
                app.terminate(nil)
            } catch {
                FileHandle.standardError.write(Data("Native image check failed: \(error)\n".utf8)); exit(1)
            }
        }
        app.run()
    }
}
