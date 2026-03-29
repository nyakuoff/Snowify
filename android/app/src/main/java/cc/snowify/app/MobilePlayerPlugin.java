package cc.snowify.app;

import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import androidx.media3.common.C;
import androidx.media3.common.MediaItem;
import androidx.media3.common.PlaybackException;
import androidx.media3.common.Player;
import androidx.media3.exoplayer.ExoPlayer;

import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

/**
 * Capacitor plugin that exposes two independent ExoPlayer instances to JavaScript.
 *
 * Each player is identified by an id string ("a" or "b"), mirroring the two
 * <audio> elements used by DualAudioEngine for crossfade playback.
 *
 * JS-side NativeAudioShim delegates all HTMLAudioElement calls here.
 *
 * Events emitted (all include an "id" field):
 *   playerReady       — player prepared; includes durationMs
 *   playerTimeUpdate  — periodic position update; includes positionMs + durationMs
 *   playerEnded       — playback reached end of stream
 *   playerError       — playback error; includes message
 */
@CapacitorPlugin(name = "MobilePlayer")
public class MobilePlayerPlugin extends Plugin {

    private static final String TAG = "MobilePlayer";

    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private Runnable timeUpdateRunnable;
    private boolean listenersAdded = false;

    // ─── Player access via PlaybackService ───────────────────────────────

    private ExoPlayer getPlayer(String id) {
        PlaybackService svc = PlaybackService.instance;
        if (svc == null) {
            Log.w(TAG, "PlaybackService not ready for id=" + id);
            return null;
        }
        return svc.getPlayer(id);
    }

    /**
     * Adds event listeners to both ExoPlayer instances once, the first time
     * a player operation is requested.
     */
    private void setupListenersOnce() {
        if (listenersAdded) return;
        PlaybackService svc = PlaybackService.instance;
        if (svc == null) return;
        listenersAdded = true;
        addListenerToPlayer("a", svc.getPlayer("a"));
        addListenerToPlayer("b", svc.getPlayer("b"));
        ensureTimeUpdates();
    }

    private void addListenerToPlayer(String id, ExoPlayer player) {
        if (player == null) return;
        player.addListener(new Player.Listener() {
            @Override
            public void onPlaybackStateChanged(int state) {
                if (state == Player.STATE_READY) {
                    long durMs = player.getDuration();
                    JSObject data = new JSObject();
                    data.put("id", id);
                    data.put("durationMs", durMs == C.TIME_UNSET ? -1 : durMs);
                    notifyListeners("playerReady", data);
                } else if (state == Player.STATE_ENDED) {
                    JSObject data = new JSObject();
                    data.put("id", id);
                    notifyListeners("playerEnded", data);
                }
            }

            @Override
            public void onPlayerError(PlaybackException error) {
                Log.e(TAG, "ExoPlayer error [" + id + "]: " + error.getMessage());
                JSObject data = new JSObject();
                data.put("id", id);
                data.put("message", error.getMessage() != null ? error.getMessage() : "Playback error");
                notifyListeners("playerError", data);
            }
        });
    }

    // ─── Plugin methods ───────────────────────────────────────────────────

    @PluginMethod
    public void load(PluginCall call) {
        String id  = call.getString("id", "a");
        String url = call.getString("url");
        if (url == null || url.isEmpty()) {
            call.reject("url is required");
            return;
        }
        mainHandler.post(() -> {
            ExoPlayer player = getPlayer(id);
            if (player == null) { call.reject("service not ready"); return; }
            setupListenersOnce();
            player.stop();
            player.setMediaItem(MediaItem.fromUri(url));
            player.prepare();
            call.resolve();
        });
    }

    @PluginMethod
    public void play(PluginCall call) {
        String id = call.getString("id", "a");
        mainHandler.post(() -> {
            ExoPlayer player = getPlayer(id);
            if (player == null) { call.reject("service not ready"); return; }
            player.setPlayWhenReady(true);
            call.resolve();
        });
    }

    @PluginMethod
    public void pause(PluginCall call) {
        String id = call.getString("id", "a");
        mainHandler.post(() -> {
            ExoPlayer player = getPlayer(id);
            if (player != null) player.setPlayWhenReady(false);
            call.resolve();
        });
    }

    @PluginMethod
    public void stop(PluginCall call) {
        String id = call.getString("id", "a");
        mainHandler.post(() -> {
            ExoPlayer player = getPlayer(id);
            if (player != null) {
                player.stop();
                player.clearMediaItems();
            }
            call.resolve();
        });
    }

    @PluginMethod
    public void seekTo(PluginCall call) {
        String id       = call.getString("id", "a");
        Double posDbl   = call.getDouble("positionMs", 0.0);
        long positionMs = posDbl != null ? posDbl.longValue() : 0L;
        mainHandler.post(() -> {
            ExoPlayer player = getPlayer(id);
            if (player != null) player.seekTo(positionMs);
            call.resolve();
        });
    }

    @PluginMethod
    public void setVolume(PluginCall call) {
        String id    = call.getString("id", "a");
        float volume = call.getFloat("volume", 1.0f);
        mainHandler.post(() -> {
            ExoPlayer player = getPlayer(id);
            if (player != null) player.setVolume(Math.max(0f, Math.min(1f, volume)));
            call.resolve();
        });
    }

    @PluginMethod
    public void setNotificationMetadata(PluginCall call) {
        String title      = call.getString("title", "");
        String artist     = call.getString("artist", "");
        String artworkUrl = call.getString("artworkUrl", "");
        mainHandler.post(() -> {
            PlaybackService svc = PlaybackService.instance;
            if (svc != null) svc.updateMetadata(title, artist, artworkUrl);
            call.resolve();
        });
    }

    // ─── Plugin lifecycle ─────────────────────────────────────────────────

    @Override
    protected void handleOnDestroy() {
        stopTimeUpdates();
        // ExoPlayer instances live in PlaybackService and are released there.
    }

    // ─── Time-update polling ──────────────────────────────────────────────

    private void ensureTimeUpdates() {
        if (timeUpdateRunnable != null) return;
        timeUpdateRunnable = new Runnable() {
            @Override
            public void run() {
                PlaybackService svc = PlaybackService.instance;
                if (svc != null) {
                    for (String id : new String[]{"a", "b"}) {
                        ExoPlayer player = svc.getPlayer(id);
                        if (player != null && player.isPlaying()) {
                            long pos = player.getCurrentPosition();
                            long dur = player.getDuration();
                            JSObject data = new JSObject();
                            data.put("id", id);
                            data.put("positionMs", pos);
                            data.put("durationMs", dur == C.TIME_UNSET ? -1 : dur);
                            notifyListeners("playerTimeUpdate", data);
                        }
                    }
                }
                mainHandler.postDelayed(this, 500);
            }
        };
        mainHandler.post(timeUpdateRunnable);
    }

    private void stopTimeUpdates() {
        if (timeUpdateRunnable != null) {
            mainHandler.removeCallbacks(timeUpdateRunnable);
            timeUpdateRunnable = null;
        }
    }
}
