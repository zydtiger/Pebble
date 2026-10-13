import Foundation

/// Delimiters are recognized before Markdown can consume their backslashes.
enum ReplyMathToken: Equatable {
    case text(String)
    case formula(String, display: Bool)
    case literal(String)

    var source: String {
        switch self {
        case .text(let source), .literal(let source), .formula(let source, _): source
        }
    }

    static func parse(_ source: String) -> [Self] {
        var tokens: [Self] = []
        var cursor = source.startIndex
        var textStart = cursor
        while cursor < source.endIndex {
            if source[cursor] == "`", let end = codeEnd(in: source, from: cursor) {
                cursor = end
                continue
            }
            guard source[cursor] == "\\" else {
                cursor = source.index(after: cursor)
                continue
            }
            let next = source.index(after: cursor)
            guard next < source.endIndex else { break }
            let opener = source[next]
            guard opener == "(" || opener == "[" else {
                cursor = source.index(after: next)
                continue
            }
            if textStart < cursor { tokens.append(.text(String(source[textStart..<cursor]))) }
            let closer: Character = opener == "(" ? ")" : "]"
            var end = source.index(after: next)
            var closed = false
            while end < source.endIndex {
                if source[end] == "\\" {
                    let following = source.index(after: end)
                    if following < source.endIndex {
                        end = source.index(after: following)
                        if source[following] == closer { closed = true; break }
                        continue
                    }
                }
                end = source.index(after: end)
            }
            let raw = String(source[cursor..<end])
            tokens.append(closed ? .formula(raw, display: opener == "[") : .literal(raw))
            cursor = end
            textStart = end
        }
        if textStart < source.endIndex { tokens.append(.text(String(source[textStart...]))) }
        return tokens
    }

    /// Skip a complete Markdown code span, including runs containing shorter backticks.
    private static func codeEnd(in source: String, from start: String.Index) -> String.Index? {
        let count = source[start...].prefix { $0 == "`" }.count
        var cursor = source.index(start, offsetBy: count)
        while cursor < source.endIndex {
            if source[cursor] == "`" {
                let length = source[cursor...].prefix { $0 == "`" }.count
                cursor = source.index(cursor, offsetBy: length)
                if length == count { return cursor }
            } else {
                cursor = source.index(after: cursor)
            }
        }
        return nil
    }

    static func blocks(_ source: String) -> [ReplyMarkdownBlock] {
        var result: [ReplyMarkdownBlock] = []
        var prose = ""
        for token in parse(source) {
            if case .formula(let raw, display: true) = token {
                let text = prose.trimmingCharacters(in: .newlines)
                if !text.isEmpty { result.append(.prose(text)) }
                prose = ""
                result.append(.equation(raw))
            } else {
                prose += token.source
            }
        }
        if !result.isEmpty { prose = prose.trimmingCharacters(in: .newlines) }
        if !prose.isEmpty { result.append(.prose(prose)) }
        return result
    }
}
