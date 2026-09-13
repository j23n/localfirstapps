import SwiftUI

// MARK: - Design Tokens (Quiet direction)

enum Design {
    // Values come from design/tokens/gallery.toml via GalleryTokens
    // (`scripts/gen_r14.py`). Dark surfaces/ink are unauthored.
    static let accentColor  = GalleryTokens.accent
    static let accentSoft   = GalleryTokens.accent.opacity(GalleryTokens.accentSoftOpacity)

    static let bg           = GalleryTokens.bg
    static let bgCard       = GalleryTokens.bgCard
    static let bgGrouped    = GalleryTokens.bgGrouped

    static let ink          = GalleryTokens.ink
    static let ink2         = GalleryTokens.ink2
    static let ink3         = GalleryTokens.ink3
    static let separator    = GalleryTokens.separatorInk.opacity(GalleryTokens.separatorOpacity)

    static let destructive  = GalleryTokens.destructive

    static let cardRadius: CGFloat = GalleryTokens.cardRadius
    static let memoryRadius: CGFloat = GalleryTokens.memoryRadius

    /// Newsreader italic stand-in (system serif italic) — used for memory titles.
    static func serifItalic(_ size: CGFloat, weight: Font.Weight = .medium) -> Font {
        .system(size: size, weight: weight, design: .serif).italic()
    }
}

extension View {
    /// Soft gradient fade at the top edge of the enclosing scroll view so
    /// content dissolves into the nav bar (iOS 26+ — no-op on older OSes).
    @ViewBuilder
    func softTopScrollEdge() -> some View {
        if #available(iOS 26.0, *) {
            self.scrollEdgeEffectStyle(.soft, for: .top)
        } else {
            self
        }
    }

    /// Tab bar shrinks to a floating accessory while the user scrolls down
    /// and re-expands on scroll-up (iOS 26+ — no-op on older OSes). More
    /// photo per screen on the three grid tabs.
    @ViewBuilder
    func minimizableTabBar() -> some View {
        if #available(iOS 26.0, *) {
            self.tabBarMinimizeBehavior(.onScrollDown)
        } else {
            self
        }
    }
}
