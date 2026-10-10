import SwiftUI

struct GroupMembersSheet: View {
    @Bindable var model: ChatModel
    let roomID: String
    @Environment(\.dismiss) private var dismiss
    @State private var saving = false
    @State private var error: String?
    private var room: RoomRecord? { model.workspace?.rooms.first { $0.id == roomID } }
    private var available: [PersonaRecord] {
        (model.workspace?.personas ?? []).filter { persona in !(room?.members.contains { $0.personaId == persona.id } ?? false) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("People in this group.").font(.system(size: 27, design: .serif))
            Text(room?.title ?? "Group chat").font(.system(size: 13, weight: .semibold))
            Text("Adding or rejoining starts with future public messages. Removing a character stops their unfinished work here; their earlier knowledge and state stay with them.")
                .font(.system(size: 12)).foregroundStyle(PebbleTheme.muted)
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text("CURRENT MEMBERS").font(.system(size: 10, weight: .semibold)).tracking(1)
                    if room?.members.isEmpty != false { Text("No current members. Add a character below.").font(.system(size: 12)).foregroundStyle(PebbleTheme.muted) }
                    ForEach(room?.members ?? [], id: \.membershipId) { member in
                        HStack {
                            Text(label(member.personaId)).font(.system(size: 13, weight: .medium))
                            Spacer()
                            Button("Private chat & state") { dismiss(); model.select(member.personaId) }.buttonStyle(.plain)
                            Button("Remove") { change(member.personaId, adding: false) }.buttonStyle(.bordered)
                                .accessibilityIdentifier("remove-member-\(member.personaId)")
                        }
                    }
                    if !available.isEmpty {
                        Divider().padding(.vertical, 2)
                        Text("ADD OR REJOIN").font(.system(size: 10, weight: .semibold)).tracking(1)
                        ForEach(available, id: \.id) { persona in
                            HStack {
                                Text(label(persona.id)).font(.system(size: 13, weight: .medium))
                                Spacer()
                                Button(room?.formerMemberIds.contains(persona.id) == true ? "Rejoin" : "Add") { change(persona.id, adding: true) }
                                    .buttonStyle(.bordered).accessibilityIdentifier("add-member-\(persona.id)")
                            }
                        }
                    }
                }.frame(maxWidth: .infinity, alignment: .leading).padding(2)
            }.frame(maxHeight: 280)
            if let error { Text(error).font(.system(size: 11)).foregroundStyle(.red).textSelection(.enabled) }
            HStack { if saving { ProgressView().controlSize(.small) }; Spacer(); Button("Done") { dismiss() }.keyboardShortcut(.defaultAction) }
        }.padding(28).frame(width: 540).background(PebbleTheme.canvas).foregroundStyle(PebbleTheme.ink)
            .disabled(saving)
    }
    private func label(_ id: String) -> String {
        let name = model.speakerName(id)
        return (model.workspace?.personas.filter { $0.displayName == name }.count ?? 0) > 1 ? "\(name) · \(id.prefix(6))" : name
    }
    private func change(_ id: String, adding: Bool) {
        saving = true; error = nil
        Task {
            do {
                if adding { try await model.addGroupMember(id, room: roomID) }
                else { try await model.removeGroupMember(id, room: roomID) }
            } catch { self.error = model.describe(error) }
            saving = false
        }
    }
}
