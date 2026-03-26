package cc.snowify.app;

import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebView;

import com.getcapacitor.BridgeActivity;
import com.getcapacitor.BridgeWebViewClient;

import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

public class MainActivity extends BridgeActivity {

    @Override
    public void onCreate(android.os.Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        // Patch: intercept *.googlevideo.com requests from the WebView so that
        // the Web Audio API can use signed stream URLs (they lack CORS headers).
        getBridge().getWebView().setWebViewClient(new BridgeWebViewClient(getBridge()) {
            @Override
            public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
                String url = request.getUrl().toString();
                if (!url.contains(".googlevideo.com/") && !url.contains("googleusercontent.com/")) {
                    return super.shouldInterceptRequest(view, request);
                }

                try {
                    HttpURLConnection conn = (HttpURLConnection) new URL(url).openConnection();
                    conn.setRequestMethod(request.getMethod());
                    for (Map.Entry<String, String> h : request.getRequestHeaders().entrySet()) {
                        conn.setRequestProperty(h.getKey(), h.getValue());
                    }
                    conn.connect();

                    // Build response headers with CORS injected
                    Map<String, String> headers = new HashMap<>();
                    for (Map.Entry<String, List<String>> entry : conn.getHeaderFields().entrySet()) {
                        if (entry.getKey() != null && !entry.getValue().isEmpty()) {
                            headers.put(entry.getKey(), entry.getValue().get(0));
                        }
                    }
                    headers.put("Access-Control-Allow-Origin", "*");
                    headers.put("Access-Control-Allow-Headers", "*");

                    String contentType = conn.getContentType();
                    if (contentType == null) contentType = "application/octet-stream";
                    String mimeType = contentType.split(";")[0].trim();

                    int statusCode    = conn.getResponseCode();
                    String statusMsg  = conn.getResponseMessage();
                    if (statusMsg == null || statusMsg.isEmpty()) statusMsg = "OK";

                    InputStream stream = statusCode >= 400 ? conn.getErrorStream() : conn.getInputStream();
                    return new WebResourceResponse(mimeType, "UTF-8", statusCode, statusMsg, headers, stream);
                } catch (IOException e) {
                    return super.shouldInterceptRequest(view, request);
                }
            }
        });
    }
}


