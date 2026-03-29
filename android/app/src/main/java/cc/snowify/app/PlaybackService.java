package cc.snowify.app;

import android.app.PendingIntent;
import android.content.Intent;
import android.net.Uri;
import android.util.Log;

import androidx.annotation.Nullable;
import androidx.media3.common.AudioAttributes;
import androidx.media3.common.C;
import androidx.media3.common.MediaItem;
import androidx.media3.common.MediaMetadata;
import androidx.media3.datasource.DefaultHttpDataSource;
import androidx.media3.exoplayer.ExoPlayer;
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory;
import androidx.media3.session.MediaSession;
import androidx.media3.session.MediaSessionService;

/**
 * Foreground service that hosts the ExoPlayer instances and a MediaSession so
 * Android shows a persistent media notification while music is playing.
 *
 * Both ExoPlayer instances ("a" and "b") are created here to support the
 * crossfade engine.  The MediaSession is linked to player "a" (primary) so
 * the notification reflects the current track.  Call {@link #updateMetadata}
 * whenever the active track changes so the notification stays in sync.
 */
public class PlaybackService extends MediaSessionService {

    private static final String TAG = "PlaybackService";

    /** Accessed by MobilePlayerPlugin on the main thread. */
    static volatile PlaybackService instance;

    private ExoPlayer playerA;
    private ExoPlayer playerB;
    private MediaSession mediaSession;

    // ─── Service lifecycle ────────────────────────────────────────────────

    @Override
    public void onCreate() {
        super.onCreate();
        instance = this;

        DefaultHttpDataSource.Factory dsFactory = new DefaultHttpDataSource.Factory()
            .setUserAgent(MainActivity.VR_USER_AGENT)
            .setAllowCrossProtocolRedirects(true);

        playerA = buildPlayer(dsFactory);
        playerB = buildPlayer(dsFactory);

        // Tapping the notification reopens the app.
        PendingIntent launchIntent = PendingIntent.getActivity(
            this, 0,
            new Intent(this, MainActivity.class)
                .setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE
        );

        mediaSession = new MediaSession.Builder(this, playerA)
            .setSessionActivity(launchIntent)
            .build();

        Log.i(TAG, "PlaybackService created");
    }

    @Override
    @Nullable
    public MediaSession onGetSession(MediaSession.ControllerInfo controllerInfo) {
        return mediaSession;
    }

    @Override
    public void onDestroy() {
        mediaSession.release();
        playerA.release();
        playerB.release();
        instance = null;
        Log.i(TAG, "PlaybackService destroyed");
        super.onDestroy();
    }

    // ─── Helpers ──────────────────────────────────────────────────────────

    private ExoPlayer buildPlayer(DefaultHttpDataSource.Factory dsFactory) {
        ExoPlayer player = new ExoPlayer.Builder(this)
            .setMediaSourceFactory(new DefaultMediaSourceFactory(dsFactory))
            .build();
        player.setAudioAttributes(
            new AudioAttributes.Builder()
                .setUsage(C.USAGE_MEDIA)
                .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                .build(),
            /* handleAudioFocus= */ true
        );
        return player;
    }

    /**
     * Returns the ExoPlayer for the given shim id ("a" or "b").
     * Called from MobilePlayerPlugin on the main thread.
     */
    public ExoPlayer getPlayer(String id) {
        return "b".equals(id) ? playerB : playerA;
    }

    /**
     * Updates the MediaSession metadata so the notification shows the
     * correct track title, artist, and artwork.
     * Must be called on the main thread.
     */
    public void updateMetadata(String title, String artist, String artworkUrl) {
        MediaMetadata meta = new MediaMetadata.Builder()
            .setTitle(title != null ? title : "")
            .setArtist(artist != null ? artist : "")
            .setArtworkUri(artworkUrl != null && !artworkUrl.isEmpty()
                ? Uri.parse(artworkUrl) : null)
            .build();

        MediaItem current = playerA.getCurrentMediaItem();
        if (current != null) {
            int index = playerA.getCurrentMediaItemIndex();
            playerA.replaceMediaItem(index, current.buildUpon().setMediaMetadata(meta).build());
        }
    }
}
