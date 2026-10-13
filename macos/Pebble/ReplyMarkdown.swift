import SwiftUI

struct ReplyMarkdown: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(ReplyMarkdownBlock.parse(text).enumerated()), id: \.offset) { _, block in
                switch block {
                case .prose(let source):
                    NativeReplyText(source: source)
                        .fixedSize(horizontal: false, vertical: true)
                case .equation(let source):
                    ScrollView(.horizontal) {
                        NativeReplyText(source: source, equation: true)
                            .fixedSize(horizontal: true, vertical: true)
                            .padding(.vertical, 8)
                    }
                    .fixedSize(horizontal: false, vertical: true)
                case .code(let source):
                    ScrollView(.horizontal) {
                        Text(verbatim: source)
                            .font(.system(size: 13, design: .monospaced))
                            .lineSpacing(3)
                            .fixedSize(horizontal: true, vertical: true)
                            .padding(12)
                    }
                    .fixedSize(horizontal: false, vertical: true)
                    .background(PebbleTheme.ink.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
                }
            }
        }
    }
}

enum ReplyMarkdownBlock: Equatable {
    case prose(String)
    case code(String)
    case equation(String)

    static func inline(_ source: String) -> AttributedString {
        (try? AttributedString(markdown: source,
                               options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(source)
    }

    static func parse(_ source: String) -> [Self] {
        var blocks: [Self] = []
        var lines: [String] = []
        var fence: Fence?

        func flushProse() {
            let text = lines.joined(separator: "\n")
            if !text.isEmpty { blocks.append(contentsOf: ReplyMathToken.blocks(text)) }
            lines.removeAll(keepingCapacity: true)
        }

        for line in source.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n") {
            if let active = fence {
                if active.closes(line) {
                    blocks.append(.code(lines.joined(separator: "\n")))
                    lines.removeAll(keepingCapacity: true)
                    fence = nil
                } else {
                    // A fence indented by up to three spaces removes that indentation from its code.
                    lines.append(String(line.dropFirst(line.prefix(active.indentation).prefix { $0 == " " }.count)))
                }
            } else if let opening = Fence(line) {
                flushProse()
                fence = opening
            } else {
                lines.append(line)
            }
        }
        if fence != nil {
            // An unfinished model reply still shows everything after its opening fence as literal code.
            blocks.append(.code(lines.joined(separator: "\n")))
        } else {
            flushProse()
        }
        return blocks
    }

    private struct Fence {
        let marker: Character
        let count: Int
        let indentation: Int

        init?(_ line: String) {
            indentation = line.prefix { $0 == " " }.count
            guard indentation <= 3 else { return nil }
            let content = line.dropFirst(indentation)
            guard let first = content.first, first == "`" || first == "~" else { return nil }
            marker = first
            count = content.prefix { $0 == first }.count
            guard count >= 3 else { return nil }
            // Backticks are not allowed in a backtick fence's optional language/info string.
            guard first != "`" || !content.dropFirst(count).contains("`") else { return nil }
        }

        func closes(_ line: String) -> Bool {
            let indentation = line.prefix { $0 == " " }.count
            guard indentation <= 3 else { return false }
            let content = line.dropFirst(indentation)
            let length = content.prefix { $0 == marker }.count
            return length >= count && content.dropFirst(length).allSatisfy { $0 == " " || $0 == "\t" }
        }
    }
}
