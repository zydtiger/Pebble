import AppKit
import SwiftUI

@main
struct PebbleApp: App {
    @State private var model: ChatModel
    private let notifications: NativeReplyNotificationSink

    init() {
        let notifications = NativeReplyNotificationSink()
        let model = ChatModel(notifications: ReplyNotifications(sink: notifications))
        self.notifications = notifications
        _model = State(initialValue: model)
        notifications.onOpenRoom = { [weak model] in model?.openNotificationRoom($0) }
    }

    var body: some Scene {
        Window("Pebble", id: "pebble") {
            ContentView(model: model)
                .background(WindowSetup(notifications: notifications))
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1000, height: 730)
        .commands {
            CommandGroup(replacing: .newItem) { }
            CommandGroup(after: .appInfo) {
                Button("Open Data Folder") { model.openFiles() }
                Button("Reload Character Files") { Task { await model.load() } }
                    .keyboardShortcut("r", modifiers: [.command, .shift])
                    .disabled(model.hasActiveReplies)
            }
        }
    }
}

private struct WindowSetup: NSViewRepresentable {
    @Environment(\.openWindow) private var openWindow
    let notifications: NativeReplyNotificationSink

    func makeNSView(context: Context) -> NSView {
        let view = ChatWindowView()
        view.notifications = notifications
        DispatchQueue.main.async {
            notifications.setWindowOpener { openWindow(id: "pebble") }
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) { }
}

private final class ChatWindowView: NSView {
    weak var notifications: NativeReplyNotificationSink?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        notifications?.chatWindow = window
        window.titlebarAppearsTransparent = true
        window.backgroundColor = NSColor(PebbleTheme.canvas)
        window.isMovableByWindowBackground = true
    }
}
