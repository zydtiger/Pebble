import Foundation
import SwiftUI

extension CoreClient {
    func activitySettings(persona: String) throws -> ActivitySettingsRecord {
        try requireBackend().activitySettings(personaId: persona)
    }
    func saveActivitySettings(persona: String, settings: ActivitySettingsRecord) throws {
        try requireBackend().saveActivitySettings(personaId: persona, settings: settings)
    }
    func activities() throws -> [PersonaActivitySnapshotRecord] {
        try requireBackend().personaActivities()
    }
    func startActivity(persona: String, objective: String) throws {
        try requireBackend().startActivity(personaId: persona, objective: objective)
    }
    func stopActivity(persona: String) throws {
        try requireBackend().stopActivity(personaId: persona)
    }
}

extension ChatModel {
    var selectedActivity: PersonaActivitySnapshotRecord? {
        selected.flatMap { personaActivities[$0.id] }
    }
    func reloadActivities() async {
        do {
            let records = try await client.activities()
            let published = records.filter { record in
                guard let previous = personaActivities[record.personaId] else { return false }
                return record.lastPublicationId != nil && previous.lastPublicationId != record.lastPublicationId
            }
            let destinations = Set(published.compactMap { record in
                workspace?.personas.first { $0.id == record.personaId }?.conversationId
            })
            personaActivities = Dictionary(uniqueKeysWithValues: records.map { ($0.personaId, $0) })
            if !destinations.isEmpty { await refreshRoomsAfterReply(notificationRooms: destinations) }
        } catch { self.error = describe(error) }
    }
    func setActivityEnabled(_ persona: PersonaRecord, enabled: Bool) {
        Task {
            do {
                var settings = try await client.activitySettings(persona: persona.id)
                settings.enabled = enabled
                try await client.saveActivitySettings(persona: persona.id, settings: settings)
                await reloadActivities()
            } catch { self.error = describe(error) }
        }
    }
    func stopActivity(_ snapshot: PersonaActivitySnapshotRecord) {
        Task {
            do {
                try await client.stopActivity(persona: snapshot.personaId)
                await reloadActivities()
            } catch { self.error = describe(error) }
        }
    }
    func startActivityObservation() {
        guard activityObservation == nil else { return }
        activityObservation = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                await self?.reloadActivities()
                do { try await Task.sleep(for: .seconds(5)) } catch { return }
            }
        }
    }
}

struct PersonaActivityControl: View {
    @Bindable var model: ChatModel
    let persona: PersonaRecord
    @State private var showing = false
    @State private var policy: ActivitySettingsRecord?
    @State private var quietHours = false
    @State private var quietStart = Calendar.current.date(from: DateComponents(hour: 23)) ?? Date()
    @State private var quietEnd = Calendar.current.date(from: DateComponents(hour: 8)) ?? Date()
    @State private var dailyBudget = 4

    private var snapshot: PersonaActivitySnapshotRecord? { model.personaActivities[persona.id] }
    private var title: String {
        guard let snapshot else { return "Private activity off" }
        if !snapshot.enabled { return "Private activity off" }
        if !snapshot.activeTaskIds.isEmpty { return "Exploring privately" }
        switch snapshot.phase {
        case "evaluating": return "Considering a private activity"
        case "assimilating": return "Reviewing a discovery"
        default: return "Private activity waiting"
        }
    }

    var body: some View {
        Button { showing.toggle() } label: {
            HStack(spacing: 5) {
                Image(systemName: snapshot?.phase == "exploring" ? "binoculars" : "clock")
                Text(title).lineLimit(1)
            }
            .font(.system(size: 10, weight: .medium))
            .foregroundStyle(.secondary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Private activity: \(title)")
        .accessibilityIdentifier("private-activity-button")
        .popover(isPresented: $showing, arrowEdge: .bottom) {
            VStack(alignment: .leading, spacing: 10) {
                Text("Private activity").font(.system(size: 14, weight: .semibold))
                Text("Explores using your Codex task settings, including web search, connected tools and task workspace access.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
                    .lineLimit(nil).fixedSize(horizontal: false, vertical: true)
                if let snapshot {
                    if !snapshot.objective.isEmpty { Text(snapshot.objective).font(.system(size: 12)) }
                    if !snapshot.summary.isEmpty { Text(snapshot.summary).font(.system(size: 11)).foregroundStyle(.secondary) }
                    if !snapshot.commitmentSummary.isEmpty {
                        Label(snapshot.commitmentSummary, systemImage: "arrow.uturn.backward")
                            .font(.system(size: 11)).foregroundStyle(.secondary)
                    }
                    if let next = snapshot.nextCheckAt {
                        Text("Next private check: \(localTime(next))").font(.system(size: 10)).foregroundStyle(.secondary)
                    }
                    Divider()
                    if policy != nil {
                        Stepper("Private checks per UTC day: \(dailyBudget)", value: $dailyBudget, in: 1...24)
                            .font(.system(size: 11))
                        Toggle("Quiet hours (local time)", isOn: $quietHours).font(.system(size: 11))
                        if quietHours {
                            DatePicker("From", selection: $quietStart, displayedComponents: .hourAndMinute)
                            DatePicker("Until", selection: $quietEnd, displayedComponents: .hourAndMinute)
                        }
                        Button("Save schedule") { saveSchedule() }
                        Divider()
                    }
                    if snapshot.enabled {
                        Button("Disable private activity") {
                            model.setActivityEnabled(persona, enabled: false)
                        }
                    } else {
                        Button("Enable private activity") { model.setActivityEnabled(persona, enabled: true) }
                    }
                    if snapshot.activityId != nil {
                        Button("Stop current activity", role: .destructive) { model.stopActivity(snapshot) }
                    }
                } else {
                    Button("Enable private activity") { model.setActivityEnabled(persona, enabled: true) }
                }
            }
            .padding(14).frame(width: 310, alignment: .leading)
            .task {
                do {
                    let settings = try await model.client.activitySettings(persona: persona.id)
                    policy = settings
                    dailyBudget = Int(settings.dailyWakeBudget)
                    quietHours = settings.quietStartMinutes != nil
                    if let start = settings.quietStartMinutes, let end = settings.quietEndMinutes {
                        quietStart = time(Int(start)); quietEnd = time(Int(end))
                    }
                } catch { model.error = model.describe(error) }
            }
        }
    }

    private func time(_ minutes: Int) -> Date {
        Calendar.current.date(from: DateComponents(hour: minutes / 60, minute: minutes % 60)) ?? Date()
    }
    private func minutes(_ date: Date) -> UInt16 {
        let parts = Calendar.current.dateComponents([.hour, .minute], from: date)
        return UInt16((parts.hour ?? 0) * 60 + (parts.minute ?? 0))
    }
    private func saveSchedule() {
        Task {
            do {
                var settings = try await model.client.activitySettings(persona: persona.id)
                settings.dailyWakeBudget = UInt8(dailyBudget)
                settings.quietStartMinutes = quietHours ? minutes(quietStart) : nil
                settings.quietEndMinutes = quietHours ? minutes(quietEnd) : nil
                try await model.client.saveActivitySettings(persona: persona.id, settings: settings)
                policy = settings
                await model.reloadActivities()
            } catch { model.error = model.describe(error) }
        }
    }

    private func localTime(_ timestamp: String) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let fractional = formatter.date(from: timestamp)
        formatter.formatOptions = [.withInternetDateTime]
        guard let date = fractional ?? formatter.date(from: timestamp) else { return "scheduled" }
        return date.formatted(date: .abbreviated, time: .shortened)
    }
}
