import Foundation

struct VCardWriter: Sendable {

    func write(_ contact: Contact) -> String {
        var lines: [String] = []
        lines.append("BEGIN:VCARD")
        lines.append("VERSION:3.0")

        // X-LOCALCONTACTS-ID
        lines.append("X-LOCALCONTACTS-ID:\(contact.localContactsID)")

        // N
        let n = [contact.familyName, contact.givenName, contact.middleName, contact.namePrefix, contact.nameSuffix]
            .map { escape($0) }
            .joined(separator: ";")
        lines.append("N:\(n)")

        // FN
        let fn = contact.fullName.isEmpty ? contact.displayName : contact.fullName
        lines.append("FN:\(escape(fn))")

        // ORG
        if !contact.organization.isEmpty {
            lines.append("ORG:\(escape(contact.organization))")
        }

        // TITLE
        if !contact.jobTitle.isEmpty {
            lines.append("TITLE:\(escape(contact.jobTitle))")
        }

        // NICKNAME
        if !contact.nickname.isEmpty {
            lines.append("NICKNAME:\(escape(contact.nickname))")
        }

        // URL
        for url in contact.urls {
            let typeParam = sanitizeType(url.label, default: "homepage")
            lines.append("URL;TYPE=\(typeParam):\(escape(url.value))")
        }

        // TEL
        for phone in contact.phoneNumbers {
            let typeParam = sanitizeType(phone.label, default: "cell")
            lines.append("TEL;TYPE=\(typeParam):\(escape(phone.value))")
        }

        // EMAIL
        for email in contact.emailAddresses {
            let typeParam = sanitizeType(email.label, default: "home")
            lines.append("EMAIL;TYPE=\(typeParam):\(escape(email.value))")
        }

        // ADR
        for addr in contact.postalAddresses {
            let typeParam = sanitizeType(addr.label, default: "home")
            let adr = [
                "", // PO box
                "", // extended address
                escape(addr.value.street),
                escape(addr.value.city),
                escape(addr.value.state),
                escape(addr.value.postalCode),
                escape(addr.value.country)
            ].joined(separator: ";")
            lines.append("ADR;TYPE=\(typeParam):\(adr)")
        }

        // BDAY
        if let bday = contact.birthday {
            if let year = bday.year, let month = bday.month, let day = bday.day {
                lines.append(String(format: "BDAY:%04d-%02d-%02d", year, month, day))
            } else if let month = bday.month, let day = bday.day {
                lines.append(String(format: "BDAY:--%02d-%02d", month, day))
            }
        }

        // PHOTO — unwrapped base64; foldLine inserts RFC 6350 continuations.
        // Do not use .lineLength76Characters: that inserts bare \n without the
        // leading space unfold requires, which corrupts photos on parse.
        if let photoData = contact.photoData {
            let base64 = photoData.base64EncodedString()
            lines.append("PHOTO;ENCODING=b;TYPE=JPEG:\(base64)")
        }

        // NOTE
        if !contact.note.isEmpty {
            lines.append("NOTE:\(escape(contact.note))")
        }

        // CATEGORIES
        if !contact.categories.isEmpty {
            let cats = contact.categories.map { escape($0) }.joined(separator: ",")
            lines.append("CATEGORIES:\(cats)")
        }

        // Unknown fields (round-trip)
        for field in contact.unknownFields {
            lines.append(field)
        }

        lines.append("END:VCARD")
        return lines.map { foldLine($0) }.joined(separator: "\r\n") + "\r\n"
    }

    func suggestedFileName(for contact: Contact) -> String {
        let name = [contact.givenName, contact.familyName]
            .filter { !$0.isEmpty }
            .joined(separator: "-")
            .lowercased()

        let sanitized = name
            .replacingOccurrences(of: "[^a-z0-9\\-]", with: "", options: .regularExpression)

        if sanitized.isEmpty {
            return "\(contact.localContactsID).vcf"
        }
        return "\(sanitized).vcf"
    }

    private func escape(_ text: String) -> String {
        text.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\n", with: "\\n")
            .replacingOccurrences(of: ",", with: "\\,")
            .replacingOccurrences(of: ";", with: "\\;")
    }

    /// TYPE params must stay a single token. Strip anything that could split
    /// the line (`\r` `\n` `;` `:`) and keep `[A-Za-z0-9\-_]`.
    private func sanitizeType(_ label: String, default defaultLabel: String) -> String {
        let allowed = Set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_")
        let filtered = String(label.filter { allowed.contains($0) })
        return filtered.isEmpty ? defaultLabel : filtered
    }

    /// RFC 6350 §3.2: no line longer than 75 octets excluding the CRLF.
    /// Continuations begin with a single SPACE (which counts toward the 75).
    private func foldLine(_ line: String) -> String {
        let bytes = Array(line.utf8)
        guard bytes.count > 75 else { return line }

        var result: [UInt8] = []
        var offset = 0
        var isFirst = true

        while offset < bytes.count {
            let limit = isFirst ? 75 : 74
            var end = min(offset + limit, bytes.count)
            // Don't split a multi-byte UTF-8 sequence.
            while end > offset && end < bytes.count && (bytes[end] & 0xC0) == 0x80 {
                end -= 1
            }
            if end == offset {
                end = min(offset + 1, bytes.count)
            }

            if !isFirst {
                result.append(contentsOf: [0x0D, 0x0A, 0x20]) // \r\n + space
            }
            result.append(contentsOf: bytes[offset..<end])
            offset = end
            isFirst = false
        }

        return String(bytes: result, encoding: .utf8) ?? line
    }
}
