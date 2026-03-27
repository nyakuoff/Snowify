package cc.snowify.app;

import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import androidx.media3.common.C;
import androidx.media3.common.MediaItem;
import androidx.media3.common.PlaybackException;
import androidx.media3.common.Player;
import androidx.media3.datasource.DefaultHttpDataSource;
import androidx.media3.exoplayer.ExoPlayer;
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory;

import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

import java.util.LinkedHashMap;
import java.util.Map;

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

    // Must match ANDROID_CONTEXT.client.userAgent in ytm-client.js
    private static final String VR_USER_AGENT =
        "com.google.android.apps.youtube.vr.oculus/1.65.10 " +
        "(Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip";

    // Preserve insertion order so time-update polling iterates consistently
    private final Map<String, ExoPlayer> players = new LinkedHashMap<>();
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private Runnable timeUpdateRunnable;

    // ─── Player lifecycle ─────────────────────────────────────────────────

    private ExoPlayer getOrCreatePlayer(String id) {
        ExoPlayer existing = players.get(id);
        if (existing != null) return existing;

        DefaultHttpDataSource.Factory dsFactory = new DefaultHttpDataSource.Factory()
            .setUserAgent(VR_USER_AGENT)
            .setAllowCrossProtocolRedirects(true);

        ExoPlayer player = new ExoPlayer.Builder(getContext())
            .setMediaSourceFactory(new DefaultMediaSourceFactory(dsFactory))
            .build();

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

        players.put(id, player);
        ensureTimeUpdates();
        return player;
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
            ExoPlayer player = getOrCreatePlayer(id);
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
            ExoPlayer player = players.get(id);
            if (player == null) { call.reject("player not initialized"); return; }
            player.setPlayWhenReady(true);
            call.resolve();
        });
    }

    @PluginMethod
    public void pause(PluginCall call) {
        String id = call.getString("id", "a");
        mainHandler.post(() -> {
            ExoPlayer player = players.get(id);
            if (player != null) player.setPlayWhenReady(false);
            call.resolve();
        });
    }

    @PluginMethod
    public void stop(PluginCall call) {
        String id = call.getString("id", "a");
        mainHandler.post(() -> {
            ExoPlayer player = players.get(id);
            if (player != null) {
                player.stop();
                player.clearMediaItems();
            }
            call.resolve();
        });
    }

    @PluginMethod
    public void seekTo(PluginCall call) {
        String id        = call.getString("id", "a");
        Double posDbl    = call.getDouble("positionMs", 0.0);
        long positionMs  = posDbl != null ? posDbl.longValue() : 0L;
        mainHandler.post(() -> {
            ExoPlayer player = players.get(id);
            if (player != null) player.seekTo(positionMs);
            call.resolve();
        });
    }

    @PluginMethod
    public void setVolume(PluginCall call) {
        String id    = call.getString("id", "a");
        float volume = call.getFloat("volume", 1.0f);
        mainHandler.post(() -> {
            ExoPlayer player = players.get(id);
            if (player != null) player.setVolume(Math.max(0f, Math.min(1f, volume)));
            call.resolve();
        });
    }

    // ─── Plugin lifecycle ─────────────────────────────────────────────────

    @Override
    protected void handleOnDestroy() {
        stopTimeUpdates();
        mainHandler.post(() -> {
            for (ExoPlayer player : players.values()) {
                player.release();
            }
            players.clear();
        });
    }

    // ─── Time-update polling ──────────────────────────────────────────────

    private void ensureTimeUpdates() {
        if (timeUpdateRunnable != null) return;
        timeUpdateRunnable = new Runnable() {
            @Override
            public void run() {
                for (Map.Entry<String, ExoPlayer> entry : players.entrySet()) {
                    ExoPlayer player = entry.getValue();
                    if (player.isPlaying()) {
                        long pos = player.getCurrentPosition();
                        long dur = player.getDuration();
                        JSObject data = new JSObject();
                        data.put("id", entry.getKey());
                        data.put("positionMs", pos);
                        data.put("durationMs", dur == C.TIME_UNSET ? -1 : dur);
                        notifyListeners("playerTimeUpdate", data);
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
