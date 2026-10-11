import SwiftUI

struct RequestParametersEditor: View {
    @Binding var text: String
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Request parameters · JSON object").font(.caption)
            Text("Backend/model support is unverified. HTTP success does not prove behavioral support.")
                .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
            TextEditor(text: $text)
                .font(.system(size: 12, design: .monospaced))
                .frame(height: 140).border(PebbleTheme.line)
                .accessibilityLabel("Request parameters JSON object")
            Text("Use typed JSON values and nested objects. {} omits optional values; false and 0 are sent explicitly. Example: {\"temperature\": 0, \"chat_template_kwargs\": {\"enable_thinking\": false}}")
                .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
            Text("Invalid values or conflicting controls cannot be saved. Sampling and enable_thinking preserve context; other template settings start a fresh context.")
                .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
        }
    }
}
