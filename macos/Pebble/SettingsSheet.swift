import SwiftUI

struct SettingsSheet: View {
    @Bindable var model: ChatModel
    let target: SettingsTarget
    @Environment(\.dismiss) private var dismiss
    @State private var showClassifier = false
    @State private var loaded = false
    @State private var loading = true
    @State private var saving = false
    @State private var error: String?
    @State private var enabled = false
    @State private var modelName = ""
    @State private var baseURL = ""
    @State private var api = "chat_completions"
    @State private var responsesSchemaFormat = "text_format"
    @State private var credential = "keep"
    @State private var replacement = ""
    @State private var environment = ""
    @State private var hasKey = false
    @State private var window = ""
    @State private var output = ""
    @State private var requestJSON = "{}"
    @State private var name = ""
    @State private var subtitle = ""
    @State private var soul = ""
    @State private var memory = ""
    private var character: Bool { target.personaID != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(character ? "Character settings" : "App defaults").font(.system(size: 23, design: .serif))
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).disabled(saving)
                Button(saving ? "Saving…" : "Save") { Task { await save() } }
                    .keyboardShortcut(.defaultAction).disabled(!loaded || saving)
            }
            if let error { Text(error).font(.system(size: 12)).foregroundStyle(.red).textSelection(.enabled) }
            if loading { ProgressView() }
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if character {
                        LabeledContent("Name") { TextField("Name", text: $name) }
                        LabeledContent("Subtitle") { TextField("Subtitle", text: $subtitle) }
                    }
                    Toggle(character ? "Override app connection" : "Connect to a model", isOn: $enabled)
                    Text(character ? "Off inherits the whole app connection, including its context and output limits." : "Off uses the local demo. Changes to app defaults require all replies to finish.")
                        .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                    if character && !enabled && !window.isEmpty {
                        Text("Inherited context_len: \(window) · max_output_tokens: \(output)")
                            .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                    }
                    if enabled {
                        LabeledContent("Model") { TextField("Model", text: $modelName) }
                        LabeledContent("API root") { TextField("https://provider.example/v1", text: $baseURL) }
                        Picker("Protocol", selection: $api) {
                            Text("Chat Completions").tag("chat_completions")
                            Text("Responses").tag("responses")
                        }
                        if api == "responses" {
                            Picker("Response schema", selection: $responsesSchemaFormat) {
                                Text("Standard · text.format").tag("text_format")
                                Text("llama.cpp · response_format").tag("response_format")
                            }
                            Text("Use Standard for Responses providers. Select the llama.cpp extension when your endpoint requires response_format for JSON Schema. Pebble always validates replies independently.")
                                .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                        }
                        RequestParametersEditor(text: $requestJSON)
                        LabeledContent("Context length") { TextField("Required total token window", text: $window) }
                        LabeledContent("Max output tokens") { TextField("Required output cap", text: $output) }
                        Text("context_len includes input and output. Input uses a conservative UTF-8 byte estimate, not exact provider token usage. The output cap must be smaller than the window.")
                            .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                        Picker("Credentials", selection: $credential) {
                            Text("Keep existing").tag("keep")
                            Text("Replace inline key").tag("replace")
                            Text("Clear authentication").tag("clear")
                            Text("Environment variable").tag("environment")
                        }
                        if credential == "replace" { SecureField("New API key", text: $replacement) }
                        if credential == "environment" { TextField("Environment variable name", text: $environment) }
                        Text(hasKey ? "An inline key is saved. Its value is never shown here." : (environment.isEmpty ? "No inline key saved." : "Environment reference: \(environment)"))
                            .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                    }
                    if character {
                        Text("SOUL").font(.caption)
                        TextEditor(text: $soul).frame(height: 130).border(PebbleTheme.line)
                        Text("MEMORY").font(.caption)
                        TextEditor(text: $memory).frame(height: 100).border(PebbleTheme.line)
                        Text("SOUL and MEMORY changes apply after you explicitly start a fresh context. Messages remain saved.")
                            .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                    }
                    Text("Settings stay in local TOML and Markdown files.")
                        .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                }.disabled(!loaded || saving).textFieldStyle(.roundedBorder).padding(.trailing, 4)
            }
            HStack {
                Button("Participation model…") { showClassifier = true }
                Button("Open data folder") { model.openFiles() }
                if character { Button("Open character folder") { model.openCharacterFiles() } }
                Spacer()
            }
        }
        .padding(24).frame(width: 560, height: character ? 690 : 560)
        .background(PebbleTheme.canvas).foregroundStyle(PebbleTheme.ink)
        .sheet(isPresented: $showClassifier) { ClassifierSheet(model: model, target: target) }
        .task { await load() }
    }
    private func load() async {
        defer { loading = false }
        do {
            let settings = try await model.readSettings(target.personaID)
            enabled = settings.chat.enabled; modelName = settings.chat.model
            baseURL = settings.chat.baseUrl; api = settings.chat.api
            responsesSchemaFormat = settings.chat.responsesSchemaFormat
            requestJSON = settings.chat.requestJson
            hasKey = settings.chat.hasApiKey; environment = settings.chat.apiKeyEnv
            window = settings.chat.contextLen.map(String.init) ?? ""
            output = settings.chat.maxOutputTokens.map(String.init) ?? ""
            if let persona = settings.persona { name = persona.displayName; subtitle = persona.subtitle; soul = persona.soul; memory = persona.memory }
            loaded = true
        } catch { self.error = model.describe(error) }
    }
    private func number(_ text: String) throws -> UInt64? {
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return nil }
        guard let value = UInt64(text) else { throw BackendError.Settings(detail: "Enter a whole number for each context limit. Limits are required for an enabled connection.") }
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
            let update = SettingsUpdateRecord(chat: ConnectionSettingsRecord(enabled: enabled, model: modelName, baseUrl: baseURL, api: api, responsesSchemaFormat: responsesSchemaFormat, requestJson: requestJSON, hasApiKey: hasKey, apiKeyEnv: environment, contextLen: enabled ? try number(window) : nil, maxOutputTokens: enabled ? try number(output) : nil), credential: change, persona: character ? PersonaSettingsRecord(displayName: name, subtitle: subtitle, soul: soul, memory: memory) : nil)
            try await model.saveSettings(target.personaID, update: update)
            replacement = ""; dismiss()
        } catch { self.error = model.describe(error) }
        saving = false
    }
}
