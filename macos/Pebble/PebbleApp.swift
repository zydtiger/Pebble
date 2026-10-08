import AppKit
import SwiftUI

@main
struct PebbleApp: App {
    @State private var model = ChatModel()

    var body: some Scene {
        Window("Pebble", id: "pebble") {
            ContentView(model: model)
                .background(WindowSetup())
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1000, height: 730)
        .commands {
            CommandGroup(replacing: .newItem) { }
            CommandGroup(after: .appInfo) {
                Button("Open Data Folder") { model.openFiles() }
                Button("Reload Character Files") { Task { await model.load() } }
                    .keyboardShortcut("r", modifiers: [.command, .shift])
                    .disabled(model.isPreviewing)
            }
        }
    }
}

private struct WindowSetup: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async {
            view.window?.titlebarAppearsTransparent = true
            view.window?.backgroundColor = NSColor(PebbleTheme.canvas)
            view.window?.isMovableByWindowBackground = true
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) { }
}
