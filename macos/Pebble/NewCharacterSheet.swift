import SwiftUI

struct NewCharacterSheet: View {
    @Bindable var model: ChatModel
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var saving = false
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Someone new").font(.system(size: 25, design: .serif))
            TextField("Character name", text: $name).textFieldStyle(.roundedBorder)
            Text("Each character has a private chat and separate SOUL.md and MEMORY.md files. Edit them in the character folder.")
                .font(.system(size: 12)).foregroundStyle(PebbleTheme.muted)
            HStack {
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("Create character") {
                    saving = true
                    Task { if await model.createCharacter(name) { dismiss() }; saving = false }
                }.disabled(saving || name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .keyboardShortcut(.defaultAction)
            }
        }.padding(28).frame(width: 400).background(PebbleTheme.canvas)
    }
}
