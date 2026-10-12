import AppKit
import SwiftUI

@MainActor
func checkDelegationCards(window: NSWindow, model: ChatModel) async throws {
    guard let persona = model.workspace?.personas.first else { throw RenderError.failed("No delegation renderer persona") }
    model.select(persona.id)
    await model.load()
    // Freeze the synthetic presentation snapshot after real subscription checks.
    model.delegationStream?.cancel(); model.delegationStream = Task {}
    model.delegationSink?.finish(); model.delegationSink = nil
    window.setContentSize(NSSize(width: 1000, height: 730))
    window.contentView = NSHostingView(rootView: ContentView(model: model))
    func fixture(_ state: DelegationState, title: String, request: TaskRequestRecord? = nil, result: String = "", detail: String = "") -> DelegatedTaskRecord {
        DelegatedTaskRecord(id: "synthetic-delegation", title: title, personaId: persona.id, conversationId: persona.conversationId,
                            state: state, activity: state == .running ? "Searching the web…" : "", detail: detail, result: result,
                            links: result.isEmpty ? [] : [TaskLinkRecord(label: "Official synthetic source", target: "https://example.org/evidence"), TaskLinkRecord(label: "answer.txt", target: "/synthetic/answer.txt")],
                            request: request, reportState: state == .completed ? "completed" : "pending", threadId: "synthetic-thread", turnId: "synthetic-turn", createdAt: "2026-10-11T12:00:00Z")
    }
    model.delegatedTasks = [fixture(.running, title: "Verify the latest release")]
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-running")
    guard model.taskActivity(persona.conversationId) else { throw RenderError.failed("Missing native room task activity") }
    if let other = model.workspace?.rooms.first(where: { $0.id != persona.conversationId }) {
        model.select(other.id); try await Task.sleep(for: .milliseconds(150))
        guard model.roomTasks.isEmpty, model.taskActivity(persona.conversationId) else { throw RenderError.failed("Task visibility/activity lost during room navigation") }
        model.select(persona.conversationId); try await Task.sleep(for: .milliseconds(150))
        guard model.roomTasks.count == 1 else { throw RenderError.failed("Task card did not survive navigation") }
        try RenderChecks.capture(window, name: "codex-navigation-return")
    }

    let question = TaskRequestRecord(id: "synthetic-question", approval: false, summary: "Codex needs your answer", questions: [TaskQuestionRecord(required: true, id: "scope", text: "Which part of the synthetic project should I inspect?", header: "Scope and consequences", description: "", guidance: [], secret: false, options: ["A", "B"], optionDescriptions: ["Read documentation only; no files change.", "Write selected code files after approval."], choices: [], multiple: false)], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsInput, title: "Review the synthetic project", request: question)]
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-question")
    window.setContentSize(NSSize(width: 820, height: 610))
    try await Task.sleep(for: .milliseconds(150)); model.replyRevision += 1
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-question-compact")
    window.setContentSize(NSSize(width: 1000, height: 730))
    func answerButtonEnabled() throws -> Bool {
        guard let button = RenderChecks.findAccessible(window.contentView!, identifier: "task-send-answers") else { throw RenderError.failed("Missing task answer button") }
        let selector = NSSelectorFromString("isAccessibilityEnabled")
        guard button.responds(to: selector) else { throw RenderError.failed("Task answer button exposes no enabled state") }
        typealias Enabled = @convention(c) (AnyObject, Selector) -> Bool
        return unsafeBitCast(button.method(for: selector), to: Enabled.self)(button, selector)
    }
    guard try !answerButtonEnabled() else { throw RenderError.failed("Native required question accepted an empty answer") }
    guard RenderChecks.findAccessible(window.contentView!, identifier: "task-question-header-scope") != nil,
          let nativeOption = RenderChecks.findAccessible(window.contentView!, identifier: "task-native-option-scope-0"),
          nativeOption.perform(NSSelectorFromString("accessibilityLabel"))?.takeUnretainedValue() as? String == "A: Read documentation only; no files change.",
          RenderChecks.pressAccessible(nativeOption) else { throw RenderError.failed("Native question lost its header, option consequences or choice action") }
    try await Task.sleep(for: .milliseconds(100))
    guard try answerButtonEnabled() else { throw RenderError.failed("Choosing a described native option did not answer the question") }
    let optional = TaskQuestionRecord(required: false, id: "optional", text: "Optional note", header: "", description: "", guidance: [], secret: false, options: [], optionDescriptions: [], choices: [], multiple: false)
    let allOptional = TaskRequestRecord(id: "form-optional", approval: false, summary: "Connected tool: all fields optional", questions: [optional], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsInput, title: "Optional connected-tool form", request: allOptional)]
    try await Task.sleep(for: .milliseconds(150))
    guard try answerButtonEnabled() else { throw RenderError.failed("All-optional form prevented an empty submission") }
    try RenderChecks.capture(window, name: "codex-form-optional")
    let mixed = TaskRequestRecord(id: "form-mixed", approval: false, summary: "Connected tool: required and optional fields", questions: [TaskQuestionRecord(required: true, id: "name", text: "Required name", header: "", description: "", guidance: [], secret: false, options: [], optionDescriptions: [], choices: [], multiple: false), optional], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsInput, title: "Mixed connected-tool form", request: mixed)]
    try await Task.sleep(for: .milliseconds(150))
    guard try !answerButtonEnabled() else { throw RenderError.failed("Mixed form did not require its required field") }
    func answerFields(_ view: NSView) -> [NSTextField] {
        let own = (view as? NSTextField).map { $0.placeholderString == "Your answer" ? [$0] : [] } ?? []
        return own + view.subviews.flatMap { answerFields($0) }
    }
    guard let field = answerFields(window.contentView!).first, window.makeFirstResponder(field), let editor = field.currentEditor() as? NSTextView else { throw RenderError.failed("Required answer field cannot receive native text input") }
    editor.insertText("Synthetic name", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
    try await Task.sleep(for: .milliseconds(150))
    guard try answerButtonEnabled() else { throw RenderError.failed("Mixed form forced an optional answer after required input") }
    try RenderChecks.capture(window, name: "codex-form-mixed")
    guard let button = RenderChecks.findAccessible(window.contentView!, identifier: "task-send-answers"), RenderChecks.pressAccessible(button) else { throw RenderError.failed("Mixed form submission was not interactive") }
    try await Task.sleep(for: .milliseconds(150))
    model.error = nil
    let constrained = TaskRequestRecord(id: "form-constrained", approval: false, summary: "Connected tool: review the field constraints before submitting", questions: [
        TaskQuestionRecord(required: true, id: "age", text: "Age (integer)", header: "", description: "Eligible age for this synthetic request.", guidance: ["Minimum: 18", "Maximum: 21", "Suggested default: 18"], secret: false, options: [], optionDescriptions: [], choices: [], multiple: false),
        TaskQuestionRecord(required: false, id: "code", text: "Code (string)", header: "", description: "Optional short public label.", guidance: ["Minimum length (characters): 2", "Maximum length (characters): 4"], secret: false, options: [], optionDescriptions: [], choices: [], multiple: false)
    ], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsInput, title: "Constrained connected-tool form", request: constrained)]
    try await Task.sleep(for: .milliseconds(150))
    for identifier in ["task-question-description-age", "task-question-guidance-age-0", "task-question-guidance-age-1", "task-question-guidance-code-0", "task-question-guidance-code-1"] {
        guard RenderChecks.findAccessible(window.contentView!, identifier: identifier) != nil else { throw RenderError.failed("Constrained form hid its description/bounds: \(identifier)") }
    }
    guard let ageField = answerFields(window.contentView!).first, window.makeFirstResponder(ageField), let ageEditor = ageField.currentEditor() as? NSTextView else { throw RenderError.failed("Constrained field cannot receive native input") }
    ageEditor.insertText("17", replacementRange: NSRange(location: 0, length: ageEditor.string.utf16.count))
    try await Task.sleep(for: .milliseconds(100))
    try RenderChecks.capture(window, name: "codex-form-constrained")
    guard let rejectedSend = RenderChecks.findAccessible(window.contentView!, identifier: "task-send-answers"), RenderChecks.pressAccessible(rejectedSend) else { throw RenderError.failed("Cannot submit constrained draft") }
    try await Task.sleep(for: .milliseconds(150))
    // This presentation-only task has no live owner: the actual API rejects it,
    // exercising the same unchanged-request correction path as schema rejection.
    guard model.error != nil, ageField.stringValue == "17", try answerButtonEnabled(), window.makeFirstResponder(ageField), let correctingEditor = ageField.currentEditor() as? NSTextView else { throw RenderError.failed("Rejected form cleared its editable answer") }
    model.error = nil
    correctingEditor.insertText("19", replacementRange: NSRange(location: 0, length: correctingEditor.string.utf16.count))
    try await Task.sleep(for: .milliseconds(100))
    guard try answerButtonEnabled() else { throw RenderError.failed("Constrained form forced an optional answer") }
    let secretForm = TaskRequestRecord(id: "form-secret", approval: false, summary: "Connected tool: format and secret guidance", questions: [
        TaskQuestionRecord(required: false, id: "email", text: "Email (string)", header: "", description: "Optional contact for this request.", guidance: ["Format: email"], secret: false, options: [], optionDescriptions: [], choices: [], multiple: false),
        TaskQuestionRecord(required: false, id: "secret", text: "Secret (string)", header: "", description: "Enter this value locally.", guidance: ["Minimum length (characters): 4", "Maximum length (characters): 8", "A tool default exists; enter an explicit secret answer."], secret: true, options: [], optionDescriptions: [], choices: [], multiple: false)
    ], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsInput, title: "Format and secret constraints", request: secretForm)]
    try await Task.sleep(for: .milliseconds(150))
    guard try answerButtonEnabled(), answerFields(window.contentView!).contains(where: { $0 is NSSecureTextField }), RenderChecks.findAccessible(window.contentView!, identifier: "task-question-guidance-email-0") != nil else { throw RenderError.failed("Format, optional semantics or secure answer control was lost") }
    try RenderChecks.capture(window, name: "codex-form-secret")
    let choices = TaskRequestRecord(id: "form-choices", approval: false, summary: "Connected tool: choose a format and project areas", questions: [
        TaskQuestionRecord(required: true, id: "format", text: "Document format", header: "", description: "", guidance: [], secret: false, options: [], optionDescriptions: [], choices: [TaskChoiceRecord(label: "Markdown document", value: "md"), TaskChoiceRecord(label: "Plain text", value: "txt")], multiple: false),
        TaskQuestionRecord(required: false, id: "areas", text: "Project areas", header: "", description: "Choose the areas to inspect.", guidance: ["Minimum selections: 1", "Maximum selections: 2"], secret: false, options: [], optionDescriptions: [], choices: [TaskChoiceRecord(label: "Source files", value: "src"), TaskChoiceRecord(label: "Documentation", value: "docs")], multiple: true)
    ], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsInput, title: "Connected-tool choices", request: choices)]
    try await Task.sleep(for: .milliseconds(150))
    guard try !answerButtonEnabled() else { throw RenderError.failed("Single-select form accepted no selection") }
    for identifier in ["task-choice-format-md", "task-choice-areas-src", "task-choice-areas-docs"] {
        guard let control = RenderChecks.findAccessible(window.contentView!, identifier: identifier), RenderChecks.pressAccessible(control) else { throw RenderError.failed("Native choice is not interactive: \(identifier)") }
        try await Task.sleep(for: .milliseconds(100))
    }
    guard try answerButtonEnabled(), answerFields(window.contentView!).isEmpty else { throw RenderError.failed("Choice form did not accept selection or exposed unusable text fields") }
    try RenderChecks.capture(window, name: "codex-form-choices")
    guard let sendChoices = RenderChecks.findAccessible(window.contentView!, identifier: "task-send-answers"), RenderChecks.pressAccessible(sendChoices) else { throw RenderError.failed("Choice form cannot submit") }
    try await Task.sleep(for: .milliseconds(150)); model.error = nil
    let parsedApproval = TaskRequestRecord(id: "parsed-approval", approval: true, summary: "Codex requests permission to proceed.\nExecute a command\nRead file · Name: README.md · Path: README.md · Command: cat README.md\nSearch · Path: src · Pattern: TODO · Command: rg TODO src\nWorkspace: synthetic project", questions: [], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsApproval, title: "Review the parsed operation", request: parsedApproval)]
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-approval-actions")
    let stdinApproval = TaskRequestRecord(id: "stdin-approval", approval: true, summary: "Codex requests permission to proceed.\nSend input to an existing terminal\nCodex did not supply operation text or parsed action details.\nWorkspace: synthetic project", questions: [], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsApproval, title: "Review terminal input", request: stdinApproval)]
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-approval-stdin")
    let networkApproval = TaskRequestRecord(id: "network-approval", approval: true, summary: "Codex requests permission to proceed.\nExecute a command\nCodex did not supply operation text or parsed action details.\nNetwork host: api.example.com\nNetwork protocol: https\nWorkspace: synthetic project", questions: [], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsApproval, title: "Review managed-network access", request: networkApproval)]
    try await Task.sleep(for: .milliseconds(150))
    guard RenderChecks.findAccessible(window.contentView!, identifier: "task-allow-once") != nil,
          let networkSummary = RenderChecks.findAccessible(window.contentView!, identifier: "task-request-summary"),
          let networkText = (networkSummary.perform(NSSelectorFromString("accessibilityValue"))?.takeUnretainedValue() as? String) ?? (networkSummary.perform(NSSelectorFromString("accessibilityLabel"))?.takeUnretainedValue() as? String),
          networkText.contains("Network host: api.example.com"), networkText.contains("Network protocol: https") else { throw RenderError.failed("Managed-network approval did not expose its host/protocol before Allow") }
    try RenderChecks.capture(window, name: "codex-approval-network")
    let authorization = TaskRequestRecord(id: "synthetic-authorization", approval: true, summary: "Connected tool: complete authorization, then confirm or deny.", questions: [], authorizationUrl: "https://example.org/authorize?state=synthetic")
    model.delegatedTasks = [fixture(.needsApproval, title: "Connected-tool authorization", request: authorization)]
    try await Task.sleep(for: .milliseconds(150))
    guard RenderChecks.findAccessible(window.contentView!, identifier: "task-authorization-link") != nil else { throw RenderError.failed("Pending authorization link is unavailable") }
    try RenderChecks.capture(window, name: "codex-authorization-pending")
    model.delegatedTasks = [fixture(.running, title: "Authorization resolved; task continues")]
    try await Task.sleep(for: .milliseconds(150))
    guard RenderChecks.findAccessible(window.contentView!, identifier: "task-authorization-link") == nil else { throw RenderError.failed("Resolved authorization link remains visible") }
    model.delegatedTasks = [fixture(.running, title: "Oversized operation refused", detail: "Codex requested an approval whose full scope exceeds Pebble's native review limit. No permission was granted.")]
    try await Task.sleep(for: .milliseconds(150))
    guard RenderChecks.findAccessible(window.contentView!, identifier: "task-allow-once") == nil else { throw RenderError.failed("Oversized scope exposed an Allow action") }
    try RenderChecks.capture(window, name: "codex-approval-oversized")
    let approval = TaskRequestRecord(id: "synthetic-approval", approval: true, summary: "Codex requests one scoped write.\nCreate answer.txt inside the selected workspace.\nEarlier effects remain if the task is cancelled.", questions: [], authorizationUrl: nil)
    model.delegatedTasks = [fixture(.needsApproval, title: "Write the requested artifact", request: approval)]
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-approval")
    model.delegatedTasks = [fixture(.completed, title: "Synthetic evidence and artifact saved", result: "Verified synthetic result from the official source. The requested answer.txt artifact is saved.")]
    try await Task.sleep(for: .milliseconds(150))
    try RenderChecks.capture(window, name: "codex-completed-collapsed")
    guard let details = RenderChecks.findAccessible(window.contentView!, identifier: "task-details-synthetic-delegation"), RenderChecks.pressAccessible(details) else { throw RenderError.failed("Cannot expand saved task result") }
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-completed")
    window.setContentSize(NSSize(width: 820, height: 610))
    try await Task.sleep(for: .milliseconds(150)); model.replyRevision += 1
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-completed-compact")
    window.setContentSize(NSSize(width: 1000, height: 730))
    var failedReport = fixture(.completed, title: "Execution saved, explanation needs recovery", result: "Verified stored evidence.")
    failedReport.reportState = "failed"
    model.delegatedTasks = [failedReport]
    let reportBubble = ChatBubble(MessageRecord(id: "synthetic-failed-report", conversationId: persona.conversationId, sequence: 999, senderKind: "persona", senderId: persona.id, content: "", status: .failed, createdAt: "2026-10-11T12:00:00Z", failureDetail: "Explanation failed; stored task result remains available.", canRetry: false, images: []))
    try await Task.sleep(for: .milliseconds(150))
    guard !model.canRetry(reportBubble), RenderChecks.findAccessible(window.contentView!, identifier: "task-retry-report-synthetic-delegation") != nil else { throw RenderError.failed("Failed report offered ordinary Retry or hid its task-card recovery") }
    try RenderChecks.capture(window, name: "codex-report-retry")
    for reportState in ["pending", "queued", "completed"] {
        var waitingReport = fixture(.completed, title: "Saved execution with explanation status", result: "Stored evidence remains available.")
        waitingReport.reportState = reportState
        model.delegatedTasks = [waitingReport]
        try await Task.sleep(for: .milliseconds(150))
        let hasFollowup = RenderChecks.findAccessible(window.contentView!, identifier: "task-followup-synthetic-delegation") != nil
        guard hasFollowup == (reportState == "completed") else { throw RenderError.failed("Followup control disagrees with report eligibility: \(reportState)") }
        if reportState == "queued" { try RenderChecks.capture(window, name: "codex-report-waiting") }
    }
    var unavailable = fixture(.completed, title: "Original membership no longer eligible", result: "Stored evidence remains available.")
    unavailable.reportState = "destination_unavailable"
    model.delegatedTasks = [unavailable]
    try await Task.sleep(for: .milliseconds(150))
    guard RenderChecks.findAccessible(window.contentView!, identifier: "task-retry-report-synthetic-delegation") == nil else { throw RenderError.failed("Lost original membership offered an impossible retry") }
    try RenderChecks.capture(window, name: "codex-destination-unavailable")
    model.delegatedTasks = [fixture(.interrupted, title: "Task interrupted on restart", detail: "Pebble stopped before a confirmed outcome. Earlier effects remain; reconcile the saved thread.")]
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-interrupted")
    model.delegatedTasks = [fixture(.failed, title: "Task could not start", detail: "Codex is unavailable. Select the installed executable in Codex settings.")]
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-failed")
    model.delegatedTasks = [fixture(.uncertain, title: "Saved task needs reconciliation", detail: "Codex disconnected before a confirmed outcome. Reconcile the saved thread; this work will not be resent.")]
    try await Task.sleep(for: .milliseconds(150)); try RenderChecks.capture(window, name: "codex-uncertain")
    model.showCodexSettings = true
    try await Task.sleep(for: .milliseconds(250))
    guard let sheet = window.attachedSheet else { throw RenderError.failed("Codex settings did not open") }
    try RenderChecks.capture(sheet, name: "codex-settings")
    model.showCodexSettings = false; model.delegatedTasks = []; model.delegationStream = nil
    try await Task.sleep(for: .milliseconds(150))
    print("Native Codex cards rendered: labelled running/activity, question, approval, completion and uncertain recovery, plus separate settings.")
}
