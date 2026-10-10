import SwiftUI

struct ClassifierSheet: View {
    @Bindable var model: ChatModel
    let target: SettingsTarget
    @Environment(\.dismiss) private var dismiss
    @State private var loaded = false
    @State private var saving = false
    @State private var error: String?
    @State private var enabled = false
    @State private var modelName = ""
    @State private var baseURL = ""
    @State private var api = "chat_completions"
    @State private var dialect = "text_format"
    @State private var profile = ""
    @State private var credential = "keep"
    @State private var replacement = ""
    @State private var environment = ""
    @State private var hasKey = false
    private var character: Bool { target.personaID != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Participation model").font(.system(size: 23, design: .serif))
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).disabled(saving)
                Button(saving ? "Saving…" : "Save") { Task { await save() } }.keyboardShortcut(.defaultAction).disabled(!loaded || saving)
            }
            Text(character ? "Character override · \(model.speakerName(target.personaID!))" : "App defaults · shared by characters without an override")
                .font(.system(size: 12)).foregroundStyle(PebbleTheme.muted)
            if let error { Text(error).font(.system(size: 12)).foregroundStyle(.red).textSelection(.enabled) }
            if !loaded { ProgressView() }
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    Text("A separate small model helps each character decide when to join a group. It chooses participation; the character's chat model writes the reply.")
                        .font(.system(size: 12))
                    Toggle(character ? "Override app participation connection" : "Enable automatic participation", isOn: $enabled)
                    Text(character ? "Off inherits the app participation connection." : "Off keeps group messages and direct mentions available, without automatic decisions.")
                        .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                    if enabled {
                        LabeledContent("Model") { TextField("Qwen/Qwen3.5-2B", text: $modelName) }
                        LabeledContent("API root") { TextField("http://localhost:8080/v1", text: $baseURL) }
                        Picker("Protocol", selection: $api) {
                            Text("Chat Completions").tag("chat_completions")
                            Text("Responses").tag("responses")
                        }
                        if api == "responses" {
                            Picker("Response schema", selection: $dialect) {
                                Text("Standard · text.format").tag("text_format")
                                Text("llama.cpp · response_format").tag("response_format")
                            }
                        } else {
                            Picker("Request mode", selection: $profile) {
                                Text("Standard protocol").tag("")
                                Text("Qwen · non-thinking").tag("qwen_non_thinking")
                            }
                            Text("Choose Qwen only for an endpoint supporting its non-thinking request extension.")
                                .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                        }
                        Picker("Credentials", selection: $credential) {
                            Text("Keep existing").tag("keep")
                            Text("Replace inline key").tag("replace")
                            Text("Clear authentication").tag("clear")
                            Text("Environment variable").tag("environment")
                        }
                        if credential == "replace" { SecureField("New API key", text: $replacement) }
                        if credential == "environment" { TextField("Environment variable name", text: $environment) }
                        Text(hasKey ? "An inline key is saved; its value is never shown." : (environment.isEmpty ? "No inline key saved." : "Environment reference: \(environment)"))
                            .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                    }
                    Text("Mentions always request a reply directly. A failed participation check stays visible so you can mention the character yourself.")
                        .font(.system(size: 12))
                    Text("Finish or stop this character's replies and participation checks before saving; app defaults require all characters to be idle.")
                        .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                }.disabled(!loaded || saving).textFieldStyle(.roundedBorder).padding(.trailing, 4)
            }
        }
        .padding(24).frame(width: 600, height: 600).background(PebbleTheme.canvas).foregroundStyle(PebbleTheme.ink)
        .task { await load() }
    }
    private func load() async {
        do {
            let settings = try await model.readClassifierSettings(target.personaID)
            enabled = settings.connection.enabled; modelName = settings.connection.model; baseURL = settings.connection.baseUrl
            api = settings.connection.api; dialect = settings.connection.responsesSchemaFormat; profile = settings.requestProfile
            hasKey = settings.connection.hasApiKey; environment = settings.connection.apiKeyEnv; loaded = true
        } catch { self.error = model.describe(error) }
    }
    private func save() async {
        saving = true; error = nil
        do {
            let change: CredentialUpdate
            switch credential {
            case "replace": change = .replace(value: replacement)
            case "clear": change = .clear
            case "environment": change = .environment(name: environment)
            default: change = .keep
            }
            let update = ClassifierSettingsRecord(connection: ConnectionSettingsRecord(enabled: enabled, model: modelName, baseUrl: baseURL, api: api, responsesSchemaFormat: dialect, hasApiKey: hasKey, apiKeyEnv: environment), requestProfile: profile)
            try await model.saveClassifierSettings(target.personaID, update: update, credential: change)
            replacement = ""; dismiss()
        } catch { self.error = model.describe(error) }
        saving = false
    }
}
