import SwiftUI

struct ParticipationPanel: View {
    @Bindable var model: ChatModel
    @State private var contentHeight: CGFloat = 0
    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack {
                Text("WHO'S JOINING").font(.system(size: 9, weight: .semibold)).tracking(1)
                Spacer()
                Button("Participation settings") { model.classifierTarget = SettingsTarget(personaID: nil) }
                    .font(.system(size: 10)).buttonStyle(.plain)
            }
            if model.participation.isEmpty {
                Text("Characters can choose to join. Mention someone to request a reply directly.")
                    .font(.system(size: 11))
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 7) {
                        ForEach(model.participation, id: \.id) { record in
                            VStack(alignment: .leading, spacing: 3) {
                                HStack(spacing: 8) {
                                    Text(model.speakerName(record.personaId)).fontWeight(.medium)
                                    if record.messageId != model.bubbles.last(where: { $0.isUser })?.id { Text("Earlier request").font(.system(size: 9)) }
                                    if record.slotStatus == "pending" { ProgressView().controlSize(.mini) }
                                    Text(label(record))
                                    Spacer()
                                    if record.slotStatus == "pending" {
                                        Button("Stop waiting") { model.cancelParticipation(record) }.buttonStyle(.plain)
                                    }
                                    if ["unconfigured", "failed", "cancelled", "interrupted"].contains(record.status) {
                                        Button("Mention") { if !model.mentionedIDs.contains(record.personaId) { model.toggleMention(record.personaId) } }
                                            .buttonStyle(.plain).accessibilityIdentifier("mention-recovery-\(record.id)")
                                        Button("Settings") { model.classifierTarget = SettingsTarget(personaID: record.personaId) }.buttonStyle(.plain)
                                    }
                                }
                                if !record.error.isEmpty { Text(record.error).foregroundStyle(record.status == "failed" ? .red : PebbleTheme.muted).textSelection(.enabled) }
                            }
                        }
                    }.font(.system(size: 10)).padding(.trailing, 4)
                        .background(GeometryReader { geometry in
                            Color.clear.preference(key: ParticipationHeight.self, value: geometry.size.height)
                        })
                }
                .frame(height: min(contentHeight, 110))
                .onPreferenceChange(ParticipationHeight.self) { contentHeight = $0 }
            }
        }
        .foregroundStyle(PebbleTheme.muted).padding(.horizontal, 30).padding(.vertical, 12)
        .background(PebbleTheme.sage.opacity(0.05))
    }
    private func label(_ record: ParticipationRecord) -> String {
        if record.slotStatus == "cancelled" { return "Reply stopped" }
        if record.slotStatus == "failed" { return "Reply failed" }
        if record.slotStatus == "interrupted" { return "Reply interrupted" }
        if record.slotStatus == "completed" { return record.phase == "permission" ? "Acknowledged" : "Replied" }
        if record.slotStatus == "admitted" { return "Reply in progress" }
        if record.slotStatus == "pending" {
            if record.status != "pending" { return "Waiting for earlier replies…" }
            if record.phase == "intent" { return "Checking the group question…" }
            if record.phase == "permission" { return "Considering an acknowledgment…" }
            return "Deciding…"
        }
        return switch record.status {
        case "pending": "Deciding…"
        case "listening": "Listening"
        case "mentioned", "selected": "Reply requested"
        case "unconfigured": "Set up participation, or mention directly"
        case "failed": "Could not decide"
        case "cancelled": "Stopped checking"
        case "interrupted": "Checking interrupted"
        default: ""
        }
    }
}

private enum ParticipationHeight: PreferenceKey {
    static var defaultValue: CGFloat { 0 }
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}
