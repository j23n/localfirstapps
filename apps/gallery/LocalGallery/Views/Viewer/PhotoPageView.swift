import SwiftUI
import UIKit
import AVFoundation
import AVKit

// MARK: - Photo Page

struct PhotoPageView: View {
    let photo: PhotoFile
    var initialThumbnail: UIImage? = nil
    @Environment(GalleryStore.self) private var store
    @Binding var isChromeVisible: Bool
    @Binding var isInfoOpen: Bool

    @State private var thumbnail: UIImage?
    @State private var fullImage: UIImage?
    @State private var videoPlayer: AVPlayer?
    @State private var isPlayingVideo = false
    @State private var isPlayingLive = false
    @State private var livePlayer: AVPlayer?
    @State private var isZoomed = false

    var body: some View {
        GeometryReader { geo in
            ZStack {
                if photo.isVideo {
                    if isPlayingVideo, let player = videoPlayer {
                        VideoPlayer(player: player)
                    } else {
                        if let img = thumbnail ?? initialThumbnail {
                            Image(uiImage: img)
                                .resizable()
                                .aspectRatio(contentMode: .fit)
                        } else {
                            ProgressView().tint(.white)
                        }

                        Button {
                            let player = AVPlayer(url: photo.url)
                            videoPlayer = player
                            isPlayingVideo = true
                            player.play()
                            isChromeVisible = false
                        } label: {
                            Image(systemName: "play.circle.fill")
                                .font(.system(size: 64))
                                .foregroundStyle(.white.opacity(0.9))
                                .shadow(radius: 8)
                        }
                    }
                } else {
                    if let displayImage = fullImage ?? thumbnail ?? initialThumbnail {
                        ZoomableImageView(
                            image: displayImage,
                            isZoomEnabled: !isInfoOpen,
                            bottomAlign: isInfoOpen,
                            onSingleTap: {
                                // Tap on the photo only toggles chrome when
                                // info is closed — when open, chrome is forced
                                // visible by the parent and tapping is a no-op.
                                guard !isInfoOpen else { return }
                                withAnimation { isChromeVisible.toggle() }
                            },
                            onZoomChange: { zoomed in
                                isZoomed = zoomed
                                if zoomed {
                                    stopLivePlayback()
                                }
                            }
                        )
                    } else {
                        ProgressView().tint(.white)
                    }

                    // Live photo video overlay
                    if isPlayingLive, let player = livePlayer {
                        AVPlayerLayerView(player: player)
                            .allowsHitTesting(false)
                    }
                }

            }
            .frame(width: geo.size.width, height: geo.size.height)
            // Live-photo press-and-hold. The `pressing` callback fires
            // *immediately* on touch start (before `minimumDuration`), so
            // anything heavyweight (start playback, hide chrome) MUST live in
            // `perform` — the recognised long-press — or every quick tap on a
            // live photo races the chrome toggle: pressing(true) hides
            // chrome, then ZoomableImageView's delayed singleTap toggles it
            // back. `pressing(false)` only stops playback, never starts it.
            .onLongPressGesture(
                minimumDuration: 0.3,
                maximumDistance: 50,
                perform: {
                    guard let liveURL = photo.livePhotoVideoURL, !isZoomed, !isInfoOpen else { return }
                    let player = AVPlayer(url: liveURL)
                    livePlayer = player
                    player.play()
                    withAnimation(.easeIn(duration: 0.15)) { isPlayingLive = true }
                    isChromeVisible = false
                },
                onPressingChanged: { pressing in
                    if !pressing { stopLivePlayback() }
                }
            )
        }
        .task(id: photo.id) {
            await loadPhoto()
        }
        .onDisappear {
            // UIPageViewController retains neighbour pages, so without this
            // a playing video keeps its audio going after the user swipes to
            // the next photo.
            videoPlayer?.pause()
            isPlayingVideo = false
            stopLivePlayback()
        }
    }

    private func loadPhoto() async {
        thumbnail = store.cachedThumbnail(for: photo.url)
        fullImage = nil
        isPlayingVideo = false
        videoPlayer?.pause()
        videoPlayer = nil
        isPlayingLive = false
        livePlayer?.pause()
        livePlayer = nil
        isZoomed = false

        if thumbnail == nil {
            thumbnail = await store.thumbnail(
                for: photo.url,
                size: CGSize(width: 400, height: 400),
                isVideo: photo.isVideo
            )
        }

        if !photo.isVideo {
            fullImage = await store.loadFullImage(for: photo.url)
        }
    }

    private func stopLivePlayback() {
        guard isPlayingLive || livePlayer != nil else { return }
        withAnimation(.easeOut(duration: 0.15)) { isPlayingLive = false }
        livePlayer?.pause()
        livePlayer = nil
    }
}

