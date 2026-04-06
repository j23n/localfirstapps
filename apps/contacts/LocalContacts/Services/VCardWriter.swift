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

        // TEL
        for phone in contact.phoneNumbers {
            let typeParam = phone.label.isEmpty ? "cell" : phone.label
            lines.append("TEL;TYPE=\(typeParam):\(phone.value)")
        }

        // EMAIL
        for email in contact.emailAddresses {
            let typeParam = email.label.isEmpty ? "home" : email.label
            lines.append("EMAIL;TYPE=\(typeParam):\(email.value)")
        }

        // ADR
        for addr in contact.postalAddresses {
            let typeParam = addr.label.isEmpty ? "home" : addr.label
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

        // PHOTO
        if let photoData = contact.photoData {
            let base64 = photoData.base64EncodedString(options: .lineLength76Characters)
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
        return lines.joined(separator: "\r\n") + "\r\n"
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
}
