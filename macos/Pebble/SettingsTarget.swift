import Foundation

struct SettingsTarget: Identifiable {
    let personaID: String?
    var id: String { personaID ?? "app" }
}
