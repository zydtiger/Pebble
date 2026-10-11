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
    @State private var credential = "keep"
    @State private var replacement = ""
    @State private var environment = ""
    @State private var hasKey = false
    @State private var window = ""
    @State private var output = ""
    @State private var requestJSON = "{}"
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
                    Text(character ? "Off inherits the whole app participation connection, including its limits." : "Off keeps group messages and direct mentions available, without automatic decisions.")
                        .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                    if character && !enabled && !window.isEmpty {
                        Text("Inherited context_len: \(window) · max_output_tokens: \(output)")
                            .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                    }
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
                        }
                        RequestParametersEditor(text: $requestJSON)
                        LabeledContent("Context length") { TextField("Required total token window", text: $window) }
                        LabeledContent("Max output tokens") { TextField("Required output cap (Qwen default: 64)", text: $output) }
                        Text("context_len includes input and output. A conservative UTF-8 byte estimate includes rules, excerpts and schema; optional older excerpts may be trimmed. Mandatory content that cannot fit fails before HTTP.")
                            .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
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
            api = settings.connection.api; dialect = settings.connection.responsesSchemaFormat; requestJSON = settings.connection.requestJson
            window = settings.connection.contextLen.map(String.init) ?? ""
            output = settings.connection.maxOutputTokens.map(String.init) ?? ""
            hasKey = settings.connection.hasApiKey; environment = settings.connection.apiKeyEnv; loaded = true
        } catch { self.error = model.describe(error) }
    }
    private func number(_ text: String) throws -> UInt64? {
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return nil }
        guard let value = UInt64(text) else { throw BackendError.Settings(detail: "Enter whole numbers for this connection's context and output limits.") }
        return value
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
            let update = ClassifierSettingsRecord(connection: ConnectionSettingsRecord(enabled: enabled, model: modelName, baseUrl: baseURL, api: api, responsesSchemaFormat: dialect, requestJson: requestJSON, hasApiKey: hasKey, apiKeyEnv: environment, contextLen: enabled ? try number(window) : nil, maxOutputTokens: enabled ? try number(output) : nil))
            try await model.saveClassifierSettings(target.personaID, update: update, credential: change)
            replacement = ""; dismiss()
        } catch { self.error = model.describe(error) }
        saving = false
    }
}
