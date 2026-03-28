package cc.snowify.app;

import android.net.Uri;
import android.util.Log;

import com.getcapacitor.BridgeActivity;

import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;

import fi.iki.elonen.NanoHTTPD;

public class MainActivity extends BridgeActivity {

    /** Port the local audio proxy listens on — must match bridge.js PROXY_PORT. */
    static final int PROXY_PORT = 17890;

    // Must match ANDROID_VR.client.userAgent in ytm-client.js.
    static final String VR_USER_AGENT =
        "com.google.android.apps.youtube.vr.oculus/1.65.10 " +
        "(Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip";

    @Override
    public void onCreate(android.os.Bundle savedInstanceState) {
        registerPlugin(MobilePlayerPlugin.class);
        super.onCreate(savedInstanceState);

        // Enable Chrome remote debugging via chrome://inspect.
        android.webkit.WebView.setWebContentsDebuggingEnabled(true);

        // Use normal WebView caching so frequently reused thumbnails do not
        // hammer remote hosts on mobile.
        getBridge().getWebView().getSettings().setCacheMode(
            android.webkit.WebSettings.LOAD_DEFAULT
        );

        // Allow http://127.0.0.1:17890 (local audio proxy) from https://localhost.
        // Without this Chromium blocks it as mixed content.
        getBridge().getWebView().getSettings().setMixedContentMode(
            android.webkit.WebSettings.MIXED_CONTENT_ALWAYS_ALLOW
        );

        // Start a local HTTP proxy on 127.0.0.1 so that googlevideo.com stream
        // URLs are served from localhost.  This bypasses WebView CORS entirely
        // and lets us set the correct MIME type so the <audio> element accepts
        // the response.  bridge.js rewrites every stream URL to point here.
        try {
            new AudioProxyServer(PROXY_PORT);
            Log.i("Snowify", "Audio proxy running on port " + PROXY_PORT);
        } catch (IOException e) {
            Log.e("Snowify", "Failed to start audio proxy", e);
        }
    }

    /**
     * Tiny NanoHTTPD server that proxies YouTube CDN stream URLs.
     * Bound to loopback only — not reachable from outside the device.
     *
     * Request:  GET /stream?url=<encodeURIComponent(streamUrl)>
     * Response: the proxied audio stream with CORS headers added and the
     *           correct MIME type (taken from the URL's own 'mime' param).
     */
    static final class AudioProxyServer extends NanoHTTPD {

        AudioProxyServer(int port) throws IOException {
            super("127.0.0.1", port);
            start(NanoHTTPD.SOCKET_READ_TIMEOUT, /* daemon = */ true);
        }

        @Override
        public Response serve(IHTTPSession session) {
            // NanoHTTPD URL-decodes query params automatically.
            String realUrl = session.getParms().get("url");
            String incomingRange = session.getHeaders().get("range");
            Log.d("Snowify", "Proxy request [range=" + incomingRange + "]: "
                    + (realUrl != null ? realUrl.substring(0, Math.min(200, realUrl.length())) : "null"));
            if (realUrl == null || realUrl.isEmpty()) {
                return newFixedLengthResponse(Response.Status.BAD_REQUEST,
                        MIME_PLAINTEXT, "missing url parameter");
            }

            try {
                HttpURLConnection conn = (HttpURLConnection) new URL(realUrl).openConnection();
                conn.setRequestMethod("GET");
                conn.setInstanceFollowRedirects(true);

                conn.setRequestProperty("User-Agent", VR_USER_AGENT);

                // Forward Range header so the media player can seek.
                if (incomingRange != null) conn.setRequestProperty("Range", incomingRange);

                // Do NOT forward Accept-Encoding: if we did, Java's automatic
                // transparent decompression would be disabled and the audio
                // element would receive raw gzip bytes it can't decode.
                conn.connect();

                int statusCode = conn.getResponseCode();

                // Use the URL's own 'mime' query parameter for the MIME type.
                // YouTube CDN returns Content-Type: video/mp4 even for audio-only
                // streams; the 'mime' param in the URL contains the truth.
                Uri uri = Uri.parse(realUrl);
                String mimeParam = uri.getQueryParameter("mime");
                String mimeType;
                if (mimeParam != null && !mimeParam.isEmpty()) {
                    mimeType = mimeParam.split(";")[0].trim();
                } else {
                    String ct = conn.getContentType();
                    mimeType = (ct != null) ? ct.split(";")[0].trim() : "application/octet-stream";
                }

                // Forward headers needed for range/seeking support.
                String contentRange  = conn.getHeaderField("Content-Range");
                String acceptRanges  = conn.getHeaderField("Accept-Ranges");

                Log.d("Snowify", "Proxy upstream: status=" + statusCode
                        + " mime=" + mimeType
                        + " content-range=" + contentRange
                        + " accept-ranges=" + acceptRanges);

                long contentLength = conn.getContentLengthLong();
                InputStream stream = (statusCode >= 400)
                        ? conn.getErrorStream()
                        : conn.getInputStream();

                Response.IStatus status = Response.Status.lookup(statusCode);
                if (status == null) status = Response.Status.OK;

                Response response = (contentLength >= 0)
                        ? newFixedLengthResponse(status, mimeType, stream, contentLength)
                        : newChunkedResponse(status, mimeType, stream);

                response.addHeader("Access-Control-Allow-Origin", "*");
                response.addHeader("Access-Control-Allow-Headers", "*");
                // A 206 response MUST include Content-Range; without it browsers
                // reject the response as malformed.
                if (contentRange != null) response.addHeader("Content-Range", contentRange);
                if (acceptRanges != null) response.addHeader("Accept-Ranges", acceptRanges);
                return response;

            } catch (Exception e) {
                Log.e("Snowify", "Proxy error for: " + realUrl, e);
                return newFixedLengthResponse(Response.Status.INTERNAL_ERROR,
                        MIME_PLAINTEXT, "proxy error: " + e.getMessage());
            }
        }
    }
}


