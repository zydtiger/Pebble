import AppKit
import SwiftUI

struct TaskMonitor: View {
    @Bindable var model: ChatModel
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var expanded = false
    @State private var buttonHovered = false

    var body: some View {
        VStack(alignment: .trailing, spacing: 8) {
            MonitorButton(expanded: expanded) { toggle() }
            .overlay {
                ProgressView().controlSize(.small).scaleEffect(0.8)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
            .frame(width: 28, height: 28)
            .background(PebbleTheme.canvas, in: Circle())
            .overlay(Circle().stroke(PebbleTheme.line, lineWidth: 1).allowsHitTesting(false))
            .foregroundStyle(PebbleTheme.sage)
            .shadow(color: .black.opacity(0.1), radius: 4, y: 2)
            .opacity(buttonHovered ? 0.55 : 1)
            .onHover { buttonHovered = $0 }
            .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: buttonHovered)
            if expanded {
                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(model.monitorTasks, id: \.id) { task in
                            HStack(alignment: .top, spacing: 9) {
                                ProgressView().controlSize(.small).scaleEffect(0.7)
                                    .frame(width: 14, height: 14).padding(.top, 2)
                                VStack(alignment: .leading, spacing: 5) {
                                    Text(task.title).font(.system(size: 12)).lineLimit(2)
                                    Text("\(model.speakerName(task.personaId)) · \(task.state.label)")
                                        .font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                                }
                                Spacer(minLength: 0)
                            }
                            .padding(12)
                            .accessibilityElement(children: .combine)
                            .accessibilityIdentifier("task-monitor-row-\(task.id)")
                        }
                    }
                }
                .scrollIndicators(.never)
                .frame(width: 300, height: min(CGFloat(model.monitorTasks.count) * 72, 264))
                .background(PebbleTheme.canvas, in: RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12).stroke(PebbleTheme.line, lineWidth: 1))
                .shadow(color: .black.opacity(0.07), radius: 12, y: 5)
                .accessibilityIdentifier("task-monitor-dropdown")
                .transition(reduceMotion ? .identity : .opacity.combined(with: .offset(y: -5)))
            }
        }
        .background(MonitorDismissal(enabled: expanded) { close() })
        .onChange(of: model.selectedID) { _, _ in close() }
        .onChange(of: model.monitorTasks.isEmpty) { _, empty in if empty { close() } }
    }
    private func toggle() {
        withAnimation(reduceMotion ? nil : .easeOut(duration: 0.16)) { expanded.toggle() }
    }
    private func close() {
        withAnimation(reduceMotion ? nil : .easeOut(duration: 0.12)) { expanded = false }
    }
}

private struct MonitorButton: NSViewRepresentable {
    var expanded: Bool
    var action: () -> Void
    func makeCoordinator() -> Coordinator { Coordinator(action) }
    func makeNSView(context: Context) -> NSButton {
        let button = NSButton(title: "", target: context.coordinator, action: #selector(Coordinator.press))
        button.isBordered = false
        button.setAccessibilityIdentifier("task-monitor-button")
        button.toolTip = "Background tasks"
        return button
    }
    func updateNSView(_ button: NSButton, context: Context) {
        context.coordinator.action = action
        button.toolTip = expanded ? "Hide background tasks" : "Background tasks"
        button.setAccessibilityLabel("Background tasks · Working")
        button.setAccessibilityValue(expanded ? "Expanded" : "Collapsed")
    }
    final class Coordinator: NSObject {
        var action: () -> Void
        init(_ action: @escaping () -> Void) { self.action = action }
        @objc func press() { action() }
    }
}

// Observe without consuming outside clicks, so the normal chat remains interactive.
private struct MonitorDismissal: NSViewRepresentable {
    var enabled: Bool
    var dismiss: () -> Void
    func makeNSView(context: Context) -> DismissalView { DismissalView() }
    func updateNSView(_ view: DismissalView, context: Context) {
        view.enabled = enabled
        view.dismiss = dismiss
    }
    static func dismantleNSView(_ view: DismissalView, coordinator: ()) { view.stopObserving() }
    final class DismissalView: NSView {
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        var enabled = false
        var dismiss: (() -> Void)?
        private var monitor: Any?
        func stopObserving() {
            if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
        }
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            stopObserving()
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .keyDown]) { [weak self] event in
                guard let self, self.enabled else { return event }
                if event.type == .keyDown {
                    if event.keyCode == 53 { self.dismiss?(); return nil }
                } else if event.window !== self.window || !self.bounds.contains(self.convert(event.locationInWindow, from: nil)) {
                    self.dismiss?()
                }
                return event
            }
        }
    }
}
