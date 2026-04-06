import Foundation

struct VCardParser: Sendable {

    func parse(data: Data, fileName: String) -> Contact? {
        guard let text = String(data: data, encoding: .utf8) else { return nil }
        return parse(string: text, fileName: fileName)
    }

    func parse(string: String, fileName: String) -> Contact? {
        let unfolded = unfold(string)
        let lines = unfolded.components(separatedBy: .newlines)

        guard lines.contains(where: { $0.uppercased().hasPrefix("BEGIN:VCARD") }) else { return nil }

        let contact = Contact(fileName: fileName)
        var unknownFields: [String] = []
        var noteLines: [String] = []
        var inNote = false

        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty { continue }

            let upper = trimmed.uppercased()
            if upper == "BEGIN:VCARD" || upper == "END:VCARD" || upper.hasPrefix("VERSION:") {
                continue
            }

            guard let (field, params, value) = parseLine(trimmed) else {
                unknownFields.append(trimmed)
                continue
            }

            let fieldUpper = field.uppercased()

            switch fieldUpper {
            case "FN":
                contact.fullName = unescape(value)

            case "N":
                let parts = value.components(separatedBy: ";")
                contact.familyName = unescape(parts[safe: 0] ?? "")
                contact.givenName = unescape(parts[safe: 1] ?? "")
                contact.middleName = unescape(parts[safe: 2] ?? "")
                contact.namePrefix = unescape(parts[safe: 3] ?? "")
                contact.nameSuffix = unescape(parts[safe: 4] ?? "")

            case "TEL":
                let label = extractTypeLabel(params, default: "mobile")
                contact.phoneNumbers.append(LabeledValue(label: label, value: unescape(value)))

            case "EMAIL":
                let label = extractTypeLabel(params, default: "home")
                contact.emailAddresses.append(LabeledValue(label: label, value: unescape(value)))

            case "ADR":
                let label = extractTypeLabel(params, default: "home")
                let parts = value.components(separatedBy: ";")
                let address = PostalAddress(
                    street: unescape(parts[safe: 2] ?? ""),
                    city: unescape(parts[safe: 3] ?? ""),
                    state: unescape(parts[safe: 4] ?? ""),
                    postalCode: unescape(parts[safe: 5] ?? ""),
                    country: unescape(parts[safe: 6] ?? "")
                )
                contact.postalAddresses.append(LabeledValue(label: label, value: address))

            case "ORG":
                contact.organization = unescape(value.components(separatedBy: ";").first ?? "")

            case "TITLE":
                contact.jobTitle = unescape(value)

            case "NICKNAME":
                contact.nickname = unescape(value)

            case "URL":
                let label = extractTypeLabel(params, default: "homepage")
                contact.urls.append(LabeledValue(label: label, value: unescape(value)))

            case "BDAY":
                contact.birthday = parseBirthday(value)

            case "PHOTO":
                if let photoData = parsePhoto(value: value, params: params) {
                    contact.photoData = photoData
                } else {
                    unknownFields.append(trimmed)
                }

            case "NOTE":
                contact.note = unescape(value)

            case "CATEGORIES":
                contact.categories = value.components(separatedBy: ",").map { unescape($0.trimmingCharacters(in: .whitespaces)) }.filter { !$0.isEmpty }

            case "X-LOCALCONTACTS-ID":
                contact.localContactsID = value

            default:
                unknownFields.append(trimmed)
            }

            _ = inNote
            _ = noteLines
        }

        contact.unknownFields = unknownFields
        return contact
    }

    func parseMultiple(data: Data, fileName: String) -> [Contact] {
        guard let text = String(data: data, encoding: .utf8) else { return [] }
        let unfolded = unfold(text)

        var contacts: [Contact] = []
        var currentBlock = ""
        var inCard = false

        for line in unfolded.components(separatedBy: .newlines) {
            if line.uppercased().hasPrefix("BEGIN:VCARD") {
                inCard = true
                currentBlock = line + "\n"
            } else if line.uppercased().hasPrefix("END:VCARD") {
                currentBlock += line + "\n"
                if let contact = parse(string: currentBlock, fileName: fileName) {
                    contacts.append(contact)
                }
                currentBlock = ""
                inCard = false
            } else if inCard {
                currentBlock += line + "\n"
            }
        }

        return contacts
    }

    // MARK: - Line Parsing

    private func parseLine(_ line: String) -> (field: String, params: [String], value: String)? {
        // Handle group prefix (e.g., "item1.TEL;...")
        var workingLine = line
        if let dotIndex = workingLine.firstIndex(of: "."),
           let colonIndex = workingLine.firstIndex(of: ":"),
           dotIndex < colonIndex,
           !workingLine[workingLine.startIndex..<dotIndex].contains(";") {
            workingLine = String(workingLine[workingLine.index(after: dotIndex)...])
        }

        guard let colonIndex = workingLine.firstIndex(of: ":") else { return nil }

        let fieldAndParams = String(workingLine[workingLine.startIndex..<colonIndex])
        let value = String(workingLine[workingLine.index(after: colonIndex)...])

        let parts = fieldAndParams.components(separatedBy: ";")
        let field = parts[0]
        let params = Array(parts.dropFirst())

        return (field, params, value)
    }

    // MARK: - Helpers

    private func unfold(_ text: String) -> String {
        // vCard line folding: a line starting with space/tab is a continuation
        text.replacingOccurrences(of: "\r\n ", with: "")
            .replacingOccurrences(of: "\r\n\t", with: "")
            .replacingOccurrences(of: "\n ", with: "")
            .replacingOccurrences(of: "\n\t", with: "")
    }

    private func unescape(_ text: String) -> String {
        text.replacingOccurrences(of: "\\n", with: "\n")
            .replacingOccurrences(of: "\\N", with: "\n")
            .replacingOccurrences(of: "\\,", with: ",")
            .replacingOccurrences(of: "\\;", with: ";")
            .replacingOccurrences(of: "\\\\", with: "\\")
    }

    private func extractTypeLabel(_ params: [String], default defaultLabel: String) -> String {
        for param in params {
            let upper = param.uppercased()
            if upper.hasPrefix("TYPE=") {
                let types = param.dropFirst(5).components(separatedBy: ",")
                // Prefer specific types, skip "pref"
                for type in types {
                    let t = type.trimmingCharacters(in: .whitespaces).lowercased()
                    if t != "pref" && t != "voice" && t != "internet" {
                        return t
                    }
                }
            }
            // Bare type labels (vCard 2.1 style)
            let bare = param.uppercased()
            if ["HOME", "WORK", "CELL", "MOBILE", "FAX", "PAGER", "MAIN", "IPHONE", "OTHER"].contains(bare) {
                return param.lowercased()
            }
        }
        return defaultLabel
    }

    private func parseBirthday(_ value: String) -> DateComponents? {
        let cleaned = value.replacingOccurrences(of: "-", with: "")
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")

        // Try YYYYMMDD
        if cleaned.count == 8 {
            formatter.dateFormat = "yyyyMMdd"
            if let date = formatter.date(from: cleaned) {
                return Calendar.current.dateComponents([.year, .month, .day], from: date)
            }
        }

        // Try with dashes: YYYY-MM-DD
        formatter.dateFormat = "yyyy-MM-dd"
        if let date = formatter.date(from: value) {
            return Calendar.current.dateComponents([.year, .month, .day], from: date)
        }

        // Try --MM-DD (no year)
        if value.hasPrefix("--") {
            let noYear = value.dropFirst(2).replacingOccurrences(of: "-", with: "")
            if noYear.count == 4, let month = Int(noYear.prefix(2)), let day = Int(noYear.suffix(2)) {
                return DateComponents(month: month, day: day)
            }
        }

        return nil
    }

    private func parsePhoto(value: String, params: [String]) -> Data? {
        let isBase64 = params.contains(where: {
            $0.uppercased().contains("BASE64") || $0.uppercased().contains("ENCODING=B") || $0.uppercased().contains("ENCODING=BASE64")
        })
        if isBase64 {
            let cleaned = value.replacingOccurrences(of: "\\s+", with: "", options: .regularExpression)
            return Data(base64Encoded: cleaned)
        }
        return nil
    }
}

extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
