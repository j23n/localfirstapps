import AVFoundation
import UIKit

/// Progress for host metadata enrichment requested by `MusicSession`.
struct ScanProgress: Sendable, Equatable {
    var completed: Int
    var total: Int
}

struct HostTrackMetadata: Sendable {
    let track: Track
    let result: MetadataResult
}

enum MetadataLoaderError: LocalizedError, Sendable {
    case identityMismatch(coreID: String, swiftID: String)

    var errorDescription: String? {
        switch self {
        case .identityMismatch(let coreID, let swiftID):
            return "Music identity mismatch (core \(coreID), Swift \(swiftID))"
        }
    }
}

struct MetadataLoader {
    /// AVFoundation remains an iOS host port. The result sent back to Rust is
    /// the generated metadata command DTO; `Track` stays a local UI adapter.
    static func loadMetadata(for request: MetadataRequest) async throws -> HostTrackMetadata {
        let url = URL(fileURLWithPath: request.path)
        let swiftID = Track.stableID(for: url)
        guard swiftID.uuidString.caseInsensitiveCompare(request.id) == .orderedSame else {
            throw MetadataLoaderError.identityMismatch(
                coreID: request.id,
                swiftID: swiftID.uuidString.lowercased()
            )
        }
        let asset = AVURLAsset(url: url)
        let fallbackTitle = url.deletingPathExtension().lastPathComponent

        var title = fallbackTitle
        var artist = "Unknown Artist"
        var album = "Unknown Album"
        var duration: Double = 0
        var artworkData: Data?

        do {
            let durationTime = try await asset.load(.duration)
            duration = CMTimeGetSeconds(durationTime)
            if duration.isNaN || duration.isInfinite { duration = 0 }
        } catch { }

        do {
            let metadata = try await asset.load(.commonMetadata)

            for item in metadata {
                guard let key = item.commonKey else { continue }
                switch key {
                case .commonKeyTitle:
                    if let value = try? await item.load(.stringValue), !value.isEmpty {
                        title = value
                    }
                case .commonKeyArtist:
                    if let value = try? await item.load(.stringValue), !value.isEmpty {
                        artist = value
                    }
                case .commonKeyAlbumName:
                    if let value = try? await item.load(.stringValue), !value.isEmpty {
                        album = value
                    }
                case .commonKeyArtwork:
                    if let data = try? await item.load(.dataValue) {
                        artworkData = data
                    }
                default:
                    break
                }
            }
        } catch { }

        // Persist artwork to disk cache instead of the in-memory Track.
        var hasArtwork = false
        if let data = artworkData, !data.isEmpty {
            ArtworkCache.storeSync(data, for: url)
            hasArtwork = true
        } else {
            // Clean up stale artwork from a prior scan.
            if ArtworkCache.hasArtwork(for: url) {
                ArtworkCache.remove(for: url)
            }
        }

        // Persist lyrics to disk cache; only `hasLyrics` lives on the Track.
        let unsynced = await extractUnsyncedLyrics(from: asset)
        let synced = await extractSyncedLyrics(from: asset)
        let lyrics = TrackLyrics(unsynced: unsynced, synced: synced)
        var hasLyrics = false
        if !lyrics.isEmpty {
            LyricsCache.storeSync(lyrics, for: url)
            hasLyrics = true
        } else if LyricsCache.hasLyrics(for: url) {
            LyricsCache.remove(for: url)
        }

        let track = Track(
            id: swiftID,
            url: url,
            title: title,
            artist: artist,
            album: album,
            duration: duration,
            hasArtwork: hasArtwork,
            hasLyrics: hasLyrics
        )
        let durationMilliseconds = UInt64(max(0, duration * 1_000).rounded())
        return HostTrackMetadata(
            track: track,
            result: MetadataResult(
                id: request.id,
                title: title,
                artist: artist,
                album: album,
                durationMs: durationMilliseconds,
                hasArtwork: hasArtwork,
                hasLyrics: hasLyrics
            )
        )
    }

    // MARK: - Lyrics Extraction

    private static func extractUnsyncedLyrics(from asset: AVAsset) async -> String? {
        // Try iTunes metadata (©lyr)
        let iTunesFormats: [AVMetadataFormat] = [.iTunesMetadata]
        for format in iTunesFormats {
            if let items = try? await asset.loadMetadata(for: format) {
                for item in items {
                    if let key = item.identifier,
                       key == .iTunesMetadataLyrics,
                       let value = try? await item.load(.stringValue),
                       !value.isEmpty {
                        return value
                    }
                }
            }
        }

        // Try ID3 metadata (USLT)
        if let items = try? await asset.loadMetadata(for: .id3Metadata) {
            for item in items {
                if let key = item.identifier,
                   key == .id3MetadataUnsynchronizedLyric,
                   let value = try? await item.load(.stringValue),
                   !value.isEmpty {
                    return value
                }
            }
        }

        return nil
    }

    private static func extractSyncedLyrics(from asset: AVAsset) async -> [SyncedLyricLine]? {
        guard let items = try? await asset.loadMetadata(for: .id3Metadata) else { return nil }

        for item in items {
            if let key = item.identifier,
               key == .id3MetadataSynchronizedLyric,
               let data = try? await item.load(.dataValue) {
                return parseSYLT(data: data)
            }
        }
        return nil
    }

    /// Parse ID3v2 SYLT frame payload.
    /// Format: encoding(1) language(3) timestampFormat(1) contentType(1)
    ///         contentDescriptor(null-terminated) then repeated [text\0][4-byte ms timestamp]
    static func parseSYLT(data: Data) -> [SyncedLyricLine]? {
        guard data.count > 6 else { return nil }

        let encoding = data[0]
        // bytes 1-3: language (skip)
        let timestampFormat = data[4]
        // byte 5: content type (skip)

        // Skip past content descriptor (null-terminated string after the 6-byte header)
        var offset = 6
        offset = skipNullTerminatedString(in: data, from: offset, encoding: encoding)
        guard offset < data.count else { return nil }

        var lines: [SyncedLyricLine] = []

        while offset < data.count {
            // Read null-terminated text
            guard let (text, nextOffset) = readNullTerminatedString(in: data, from: offset, encoding: encoding) else {
                break
            }
            offset = nextOffset

            // Read 4-byte big-endian timestamp
            guard offset + 4 <= data.count else { break }
            let rawTimestamp = UInt32(data[offset]) << 24
                | UInt32(data[offset + 1]) << 16
                | UInt32(data[offset + 2]) << 8
                | UInt32(data[offset + 3])
            offset += 4

            let seconds: Double
            if timestampFormat == 2 {
                // Milliseconds
                seconds = Double(rawTimestamp) / 1000.0
            } else {
                // MPEG frames — treat as ms as a fallback
                seconds = Double(rawTimestamp) / 1000.0
            }

            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty {
                lines.append(SyncedLyricLine(timestamp: seconds, text: trimmed))
            }
        }

        guard !lines.isEmpty else { return nil }
        return lines.sorted { $0.timestamp < $1.timestamp }
    }

    private static func skipNullTerminatedString(in data: Data, from offset: Int, encoding: UInt8) -> Int {
        let isUTF16 = encoding == 1 || encoding == 2
        var i = offset
        if isUTF16 {
            while i + 1 < data.count {
                if data[i] == 0 && data[i + 1] == 0 { return i + 2 }
                i += 2
            }
        } else {
            while i < data.count {
                if data[i] == 0 { return i + 1 }
                i += 1
            }
        }
        return data.count
    }

    private static func readNullTerminatedString(in data: Data, from offset: Int, encoding: UInt8) -> (String, Int)? {
        let isUTF16 = encoding == 1 || encoding == 2
        var end = offset

        if isUTF16 {
            while end + 1 < data.count {
                if data[end] == 0 && data[end + 1] == 0 { break }
                end += 2
            }
            let strData = data[offset..<end]
            let swiftEncoding: String.Encoding = encoding == 2 ? .utf16BigEndian : .utf16
            let text = String(data: strData, encoding: swiftEncoding) ?? ""
            return (text, end + 2)
        } else {
            while end < data.count && data[end] != 0 {
                end += 1
            }
            let strData = data[offset..<end]
            let swiftEncoding: String.Encoding = encoding == 3 ? .utf8 : .isoLatin1
            let text = String(data: strData, encoding: swiftEncoding) ?? ""
            return (text, end + 1)
        }
    }

}
