import SwiftUI

struct DelegationCard: View {
    @Bindable var model: ChatModel
    let task: DelegatedTaskRecord
    @State private var expanded = false
    @State private var answers: [String: String] = [:]
    @State private var selections: [String: Set<String>] = [:]
    @State private var followup = ""
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                if executionProgress {
                    ProgressView().controlSize(.small)
                } else {
                    Image(systemName: statusSymbol).foregroundStyle(statusTone).accessibilityLabel(task.state.label)
                }
                VStack(alignment: .leading, spacing: 4) {
                    Text(task.title).font(.system(size: 12, weight: .semibold)).lineLimit(2)
                    Text("Codex · \(model.speakerName(task.personaId)) · \(task.state.label)").font(.system(size: 10)).foregroundStyle(statusTone)
                    if executionProgress && !task.activity.isEmpty { Text(task.activity).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted) }
                }
                Spacer(minLength: 8)
                if task.state.active && task.state != .cancellationRequested {
                    Button("Cancel task") { model.cancelTask(task) }.font(.system(size: 10))
                }
                Button { expanded.toggle() } label: { Image(systemName: expanded ? "chevron.up" : "chevron.down") }.buttonStyle(.plain).accessibilityLabel("Task details").accessibilityIdentifier("task-details-\(task.id)")
            }
            if let request = task.request { requestView(request).id(request.id) }
            if !task.detail.isEmpty { Text(task.detail).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted).textSelection(.enabled) }
            if expanded {
                if !task.result.isEmpty { Text(task.result).font(.system(size: 12)).textSelection(.enabled).lineSpacing(4) }
                ForEach(Array(task.links.enumerated()), id: \.offset) { _, link in
                    Button { model.openTaskLink(link) } label: { Label(link.label, systemImage: link.target.hasPrefix("/") ? "doc" : "link") }
                        .buttonStyle(.link).font(.system(size: 11))
                }
                if task.reportState == "queued" { Text("\(model.speakerName(task.personaId))’s explanation is queued.").font(.system(size: 10)).foregroundStyle(PebbleTheme.muted) }
                if task.reportState == "failed" || task.reportState == "destination_unavailable" {
                    Text(reportRecovery).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                    if task.reportState == "failed" { Button("Retry explanation") { model.retryTaskReport(task) }.font(.system(size: 10)).help("Retry only the explanation; this does not run the task again.").accessibilityIdentifier("task-retry-report-\(task.id)") }
                }
                if [.interrupted, .uncertain].contains(task.state), task.threadId != nil {
                    Button("Reconcile saved task") { model.reconcileTask(task) }.font(.system(size: 10))
                }
                if !task.state.active && task.state != .uncertain && task.threadId != nil && !["pending", "queued"].contains(task.reportState) {
                    HStack {
                        TextField("Follow up on this task", text: $followup).textFieldStyle(.roundedBorder).accessibilityIdentifier("task-followup-\(task.id)")
                        Button("Send") { model.followupTask(task, prompt: followup); followup = "" }.disabled(followup.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }.font(.system(size: 11))
                }
            }
        }
        .padding(13).frame(maxWidth: .infinity, alignment: .leading)
        .background(PebbleTheme.sage.opacity(0.055), in: RoundedRectangle(cornerRadius: 13))
        .overlay(RoundedRectangle(cornerRadius: 13).stroke(PebbleTheme.line.opacity(0.6), lineWidth: 1))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("delegated-task-\(task.id)")
        .onAppear { expandRecovery() }
        .onChange(of: task.state) { _, _ in expandRecovery() }
        .onChange(of: task.reportState) { _, _ in expandRecovery() }
        .onChange(of: task.request?.id) { _, _ in answers = [:]; selections = [:] }
    }
    private var executionProgress: Bool {
        [.running, .cancellationRequested].contains(task.state) && task.request == nil
    }
    private var statusSymbol: String {
        switch task.state {
        case .queued: return "clock"
        case .running: return "bolt.circle"
        case .needsInput: return "questionmark.bubble"
        case .needsApproval: return "hand.raised"
        case .cancellationRequested: return "stop.circle"
        case .completed: return "checkmark.circle.fill"
        case .failed: return "xmark.octagon.fill"
        case .cancelled: return "stop.circle.fill"
        case .interrupted: return "pause.circle.fill"
        case .uncertain: return "exclamationmark.triangle.fill"
        }
    }
    private var statusTone: Color {
        switch task.state {
        case .failed: return .red
        case .interrupted, .uncertain: return .orange
        case .queued, .cancelled, .cancellationRequested: return PebbleTheme.muted
        default: return PebbleTheme.sage
        }
    }
    private var reportRecovery: String {
        if task.reportState == "destination_unavailable" {
            return "The original membership is no longer current. The saved task status and any evidence remain here."
        }
        return task.result.isEmpty
            ? "No final reply was returned. The task-status explanation failed."
            : "Explanation failed. Saved results are available."
    }
    private func expandRecovery() {
        if [.failed, .interrupted, .uncertain].contains(task.state) || ["failed", "destination_unavailable"].contains(task.reportState) { expanded = true }
    }
    @ViewBuilder private func requestView(_ request: TaskRequestRecord) -> some View {
        Text(request.summary).font(.system(size: 11)).textSelection(.enabled).accessibilityLabel(request.summary).accessibilityIdentifier("task-request-summary")
        if request.approval {
            if let url = request.authorizationUrl {
                Button("Open connected-tool authorization") { model.openTaskLink(TaskLinkRecord(label: "Open connected-tool authorization", target: url)) }.buttonStyle(.link).accessibilityIdentifier("task-authorization-link")
            }
            HStack {
                Button("Allow once") { model.answerTask(task, approve: true, answers: []) }.accessibilityIdentifier("task-allow-once")
                Button("Deny") { model.answerTask(task, approve: false, answers: []) }
            }.font(.system(size: 11))
        } else {
            ForEach(request.questions, id: \.id) { question in
                VStack(alignment: .leading, spacing: 6) {
                    if !question.header.isEmpty { Text(question.header).font(.system(size: 11, weight: .semibold)).accessibilityIdentifier("task-question-header-\(question.id)") }
                    Text(question.required ? question.text : "\(question.text) · Optional").font(.system(size: 11))
                    if !question.description.isEmpty { Text(question.description).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted).textSelection(.enabled).accessibilityIdentifier("task-question-description-\(question.id)") }
                    ForEach(Array(question.guidance.enumerated()), id: \.offset) { index, caption in
                        Text(caption).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted).textSelection(.enabled).accessibilityIdentifier("task-question-guidance-\(question.id)-\(index)")
                    }
                    if !question.choices.isEmpty {
                        ForEach(question.choices, id: \.value) { choice in
                            if question.multiple {
                                Toggle(choice.label, isOn: Binding(get: { selections[question.id, default: []].contains(choice.value) }, set: { chosen in
                                    if chosen { selections[question.id, default: []].insert(choice.value) }
                                    else { selections[question.id, default: []].remove(choice.value) }
                                })).toggleStyle(.checkbox).accessibilityIdentifier("task-choice-\(question.id)-\(choice.value)")
                            } else {
                                Button { selections[question.id] = [choice.value] } label: {
                                    Label(choice.label, systemImage: selections[question.id, default: []].contains(choice.value) ? "largecircle.fill.circle" : "circle")
                                }.buttonStyle(.plain).accessibilityIdentifier("task-choice-\(question.id)-\(choice.value)")
                            }
                        }
                        if !question.required && !question.multiple {
                            Button("Clear selection") { selections[question.id] = [] }.font(.system(size: 10))
                        }
                    } else {
                        ForEach(Array(question.options.enumerated()), id: \.offset) { index, option in
                            Button { answers[question.id] = option } label: {
                                VStack(alignment: .leading, spacing: 3) {
                                    Label(option, systemImage: answers[question.id] == option ? "largecircle.fill.circle" : "circle")
                                    if index < question.optionDescriptions.count && !question.optionDescriptions[index].isEmpty {
                                        Text(question.optionDescriptions[index]).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                                    }
                                }.frame(maxWidth: .infinity, alignment: .leading)
                            }.buttonStyle(.plain).accessibilityLabel(option + (index < question.optionDescriptions.count ? ": " + question.optionDescriptions[index] : "")).accessibilityIdentifier("task-native-option-\(question.id)-\(index)")
                        }
                        let binding = Binding(get: { answers[question.id] ?? "" }, set: { answers[question.id] = $0 })
                        if question.secret { SecureField("Your answer", text: binding).textFieldStyle(.roundedBorder).accessibilityIdentifier("task-answer-\(question.id)") }
                        else { TextField("Your answer", text: binding).textFieldStyle(.roundedBorder).accessibilityIdentifier("task-answer-\(question.id)") }
                    }
                }
            }
            Button("Send answers") {
                model.answerTask(task, approve: nil, answers: request.questions.map { question in
                    TaskAnswerRecord(questionId: question.id, answers: question.choices.isEmpty ? [answers[question.id] ?? ""] : question.choices.filter { selections[question.id, default: []].contains($0.value) }.map(\.value))
                })
            }.accessibilityIdentifier("task-send-answers").disabled(request.questions.contains { question in
                question.required && !question.multiple && (question.choices.isEmpty ? (answers[question.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty : selections[question.id, default: []].isEmpty)
            })
        }
    }
}
