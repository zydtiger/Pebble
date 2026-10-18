import AppKit
import SwiftUI

@MainActor
func checkDelegationMonitor(window: NSWindow, model: ChatModel) async throws {
    guard let persona = model.workspace?.personas.first else { throw RenderError.failed("No monitor renderer persona") }
    model.select(persona.id)
    window.setContentSize(NSSize(width: 1000, height: 730))
    window.contentView = NSHostingView(rootView: ContentView(model: model))
    try await Task.sleep(for: .milliseconds(150))
    await model.load()
    model.delegationStream?.cancel(); model.delegationStream = Task {}
    model.delegationSink?.finish(); model.delegationSink = nil
    var classifier = try await model.readClassifierSettings(nil)
    classifier.connection.enabled = false
    try await model.saveClassifierSettings(nil, update: classifier, credential: .keep)
    model.sendMessage("Please check the latest release while we talk.")
    for _ in 0..<200 where model.isReplying { try await Task.sleep(for: .milliseconds(20)) }
    guard !model.bubbles.isEmpty, !model.isReplying else { throw RenderError.failed("Synthetic private conversation did not finish") }

    func fixture(_ state: DelegationState, id: String = "synthetic-task", report: String = "completed", title: String = "Check the latest release") -> DelegatedTaskRecord {
        DelegatedTaskRecord(id: id, title: title, personaId: persona.id, conversationId: persona.conversationId,
                            state: state, activity: "Codex is working…", detail: "Private tool log must remain hidden", result: "Saved evidence must be explained in chat",
                            links: [], request: nil, reportState: report, threadId: "synthetic-thread", turnId: "synthetic-turn", createdAt: "2026-10-11T12:00:00Z", privateDelivery: false)
    }
    func executionProgressCount(_ view: NSView) -> Int {
        let own = (view as? NSProgressIndicator).map { $0.isHidden ? 0 : 1 } ?? 0
        return own + view.subviews.reduce(0) { $0 + executionProgressCount($1) }
    }
    func hasExecutionProgress(_ view: NSView) -> Bool { executionProgressCount(view) > 0 }
    func aggregateLabel() -> String? {
        (RenderChecks.findAccessible(window.contentView!, identifier: "task-monitor-button") as? NSButton)?.accessibilityLabel()
    }
    func click(_ point: NSPoint) {
        let up = NSEvent.mouseEvent(with: .leftMouseUp, location: point, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 2, clickCount: 1, pressure: 0)!
        let down = NSEvent.mouseEvent(with: .leftMouseDown, location: point, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1)!
        NSApplication.shared.postEvent(up, atStart: true)
        NSApplication.shared.sendEvent(down)
    }
    var pointerRegion: NSView?
    func movePointer(_ point: NSPoint) async throws {
        func findRegion(_ view: NSView) -> NSView? {
            if view.identifier?.rawValue == "task-monitor-hover-region" { return view }
            return view.subviews.lazy.compactMap(findRegion).first
        }
        let region = findRegion(window.contentView!)
        let inside = region.map { $0.bounds.contains($0.convert(point, from: nil)) } ?? false
        // Deliver native tracking transitions in-process, without system event-injection permissions.
        func event(_ type: NSEvent.EventType) -> NSEvent {
            NSEvent.enterExitEvent(with: type, location: point, modifierFlags: [], timestamp: 0,
                windowNumber: window.windowNumber, context: nil, eventNumber: 0, trackingNumber: 0, userData: nil)!
        }
        if let previous = pointerRegion, previous !== region || !inside {
            previous.mouseExited(with: event(.mouseExited))
            pointerRegion = nil
        }
        if let region, inside, pointerRegion !== region {
            try requireTracking(region)
            region.mouseEntered(with: event(.mouseEntered))
            pointerRegion = region
        }
        try await Task.sleep(for: .milliseconds(200))
    }
    func requireTracking(_ region: NSView) throws {
        guard region.trackingAreas.contains(where: { $0.options.contains([.mouseEnteredAndExited, .inVisibleRect]) }) else {
            throw RenderError.failed("Monitor is missing its native hover tracking area")
        }
    }
    func monitorPoint() throws -> NSPoint {
        guard let control = RenderChecks.findAccessible(window.contentView!, identifier: "task-monitor-button") as? NSButton else {
            throw RenderError.failed("Missing monitor button")
        }
        return control.convert(NSPoint(x: control.bounds.midX, y: control.bounds.midY), to: nil)
    }
    func hoverMonitor() async throws { try await movePointer(monitorPoint()) }
    func leaveMonitor() async throws { try await movePointer(NSPoint(x: 250, y: 50)) }
    func dropdownVisible() -> Bool {
        (RenderChecks.findAccessible(window.contentView!, identifier: "task-monitor-button") as? NSButton)?.accessibilityValue() as? String == "Expanded"
    }
    func assertNoTaskControls() throws {
        for identifier in ["delegated-task-synthetic-task", "task-followup-synthetic-task", "task-retry-report-synthetic-task", "task-allow-once", "task-send-answers", "task-request-summary"] {
            guard RenderChecks.findAccessible(window.contentView!, identifier: identifier) == nil else { throw RenderError.failed("Obsolete task control remains: \(identifier)") }
        }
    }
    window.acceptsMouseMovedEvents = true
    NSApplication.shared.activate()
    try await leaveMonitor()
    model.delegatedTasks = [fixture(.running)]
    try await Task.sleep(for: .milliseconds(200))
    guard hasExecutionProgress(window.contentView!), !dropdownVisible() else { throw RenderError.failed("Running monitor has no progress or opens automatically") }
    try assertNoTaskControls()
    try RenderChecks.capture(window, name: "task-monitor-private-closed")
    try await hoverMonitor()
    guard dropdownVisible() else { throw RenderError.failed("Pointer hover did not open the monitor") }
    let indicator = try monitorPoint()
    try await movePointer(NSPoint(x: indicator.x, y: indicator.y - 18))
    guard dropdownVisible() else { throw RenderError.failed("Crossing the indicator/menu gap closed the dropdown") }
    try await movePointer(NSPoint(x: indicator.x - 80, y: indicator.y - 55))
    guard dropdownVisible() else { throw RenderError.failed("Moving into the dropdown closed it") }
    try await hoverMonitor()
    click(try monitorPoint())
    try await Task.sleep(for: .milliseconds(200))
    guard dropdownVisible() else { throw RenderError.failed("Clicking a hovered indicator toggled the dropdown closed") }

    try RenderChecks.capture(window, name: "task-monitor-private-open")
    guard dropdownVisible(), executionProgressCount(window.contentView!) == 2 else { throw RenderError.failed("Monitor did not open with real header and row progress") }
    let escape = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}", isARepeat: false, keyCode: 53)!
    NSApplication.shared.sendEvent(escape)
    try await Task.sleep(for: .milliseconds(200))
    guard !dropdownVisible() else { throw RenderError.failed("Escape did not close monitor") }
    // Keyboard/VoiceOver can still open the monitor without a pointer.
    try await leaveMonitor()
    (RenderChecks.findAccessible(window.contentView!, identifier: "task-monitor-button") as? NSButton)?.performClick(nil)
    try await Task.sleep(for: .milliseconds(200))
    guard dropdownVisible() else { throw RenderError.failed("Accessible activation did not open the monitor") }
    click(NSPoint(x: 250, y: 50))
    try await Task.sleep(for: .milliseconds(200))
    guard !dropdownVisible() else { throw RenderError.failed("Outside click did not close monitor") }

    try await hoverMonitor()
    try await leaveMonitor()
    guard !dropdownVisible() else { throw RenderError.failed("Pointer exit did not close the monitor") }

    for state: DelegationState in [.queued, .needsInput, .needsApproval, .failed, .interrupted, .cancelled, .uncertain, .completed, .cancellationRequested] {
        model.delegatedTasks = [fixture(state, report: "queued")]
        try await Task.sleep(for: .milliseconds(150))
        guard hasExecutionProgress(window.contentView!) == (state == .cancellationRequested) else { throw RenderError.failed("Monitor progress misrepresented \(state)") }
        let visible = RenderChecks.findAccessible(window.contentView!, identifier: "task-monitor-button") != nil
        guard visible == (state == .cancellationRequested) else { throw RenderError.failed("Non-running task retained the monitor: \(state)") }
        try assertNoTaskControls()
    }
    model.delegatedTasks = [fixture(.completed), fixture(.completed, id: "explanation-failed", report: "failed"), fixture(.cancelled)]
    try await Task.sleep(for: .milliseconds(150))
    guard model.monitorTasks.isEmpty, RenderChecks.findAccessible(window.contentView!, identifier: "task-monitor-button") == nil else { throw RenderError.failed("Completed tasks or failed explanations retained the monitor") }
    try RenderChecks.capture(window, name: "task-monitor-all-completed")
    model.delegatedTasks = [fixture(.completed), fixture(.running, id: "working")]
    try await Task.sleep(for: .milliseconds(150))
    guard model.monitorTasks.map(\.id) == ["working"], aggregateLabel() == "Background tasks · Working" else { throw RenderError.failed("Completed task was not filtered beside a running task") }
    model.delegatedTasks = [fixture(.failed, id: "old-failure")] + (0..<8).map { fixture(.completed, id: "recent-\($0)") } + [fixture(.running)]
    try await Task.sleep(for: .milliseconds(150))
    guard model.monitorTasks.map(\.id) == ["synthetic-task"], aggregateLabel() == "Background tasks · Working", hasExecutionProgress(window.contentView!) else { throw RenderError.failed("Old outcomes changed the running-only monitor") }
    try await hoverMonitor()
    guard executionProgressCount(window.contentView!) == 2 else { throw RenderError.failed("Running row and button must both show progress") }
    try RenderChecks.capture(window, name: "task-monitor-running-only")
    model.delegatedTasks = [fixture(.failed, report: "failed")]
    try await Task.sleep(for: .milliseconds(150))
    guard !hasExecutionProgress(window.contentView!), !dropdownVisible(), RenderChecks.findAccessible(window.contentView!, identifier: "task-monitor-button") == nil else { throw RenderError.failed("Failed execution or explanation retained the open monitor") }
    try RenderChecks.capture(window, name: "task-monitor-failed-hidden")
    model.delegatedTasks = [fixture(.running, title: "Review the release changes and summarize the important migration steps for this workspace")]
    try await Task.sleep(for: .milliseconds(150))
    guard !dropdownVisible() else { throw RenderError.failed("A new task reopened the dropdown automatically") }
    try await hoverMonitor()
    try RenderChecks.capture(window, name: "task-monitor-wrapped-title")
    try await leaveMonitor()

    guard await model.createGroup("Release notes", members: [persona.id]) else { throw RenderError.failed("Cannot create synthetic group") }
    for _ in 0..<100 where model.loading { try await Task.sleep(for: .milliseconds(20)) }
    model.mentionedIDs = [persona.id]
    model.draft = "Keep talking here while the release check runs."
    model.sendMessage()
    for _ in 0..<200 where model.isReplying { try await Task.sleep(for: .milliseconds(20)) }
    guard model.selectedConversation?.kind == "group", model.bubbles.contains(where: { $0.senderID == persona.id }), model.error == nil else { throw RenderError.failed("Synthetic group conversation missing its persona reply") }
    model.delegatedTasks = [fixture(.running)] // Task belongs to private chat; the monitor remains app-wide.
    try await Task.sleep(for: .milliseconds(200))
    guard model.monitorTasks.count == 1, hasExecutionProgress(window.contentView!) else { throw RenderError.failed("Cross-room monitor disappeared") }
    try RenderChecks.capture(window, name: "task-monitor-group-closed")
    try await hoverMonitor()
    try RenderChecks.capture(window, name: "task-monitor-group-open")
    window.setContentSize(NSSize(width: 820, height: 610))
    try await Task.sleep(for: .milliseconds(200))
    try RenderChecks.capture(window, name: "task-monitor-group-compact")
    try assertNoTaskControls()
    try await leaveMonitor()
    model.delegatedTasks = []
    try await Task.sleep(for: .milliseconds(150))
    guard RenderChecks.findAccessible(window.contentView!, identifier: "task-monitor-button") == nil else { throw RenderError.failed("Empty monitor retained its button") }
    model.delegationStream = nil
    print("Native task monitor checks passed: fixed private/group overlay, hover open/exit/gap traversal, accessible activation, dismissal, running-only rows, terminal and queued tasks hidden, automatic disappearance, real progress and no task controls.")
}
