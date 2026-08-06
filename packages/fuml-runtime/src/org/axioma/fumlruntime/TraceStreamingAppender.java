package org.axioma.fumlruntime;

import org.apache.log4j.AppenderSkeleton;
import org.apache.log4j.spi.LoggingEvent;

/**
 * Forwards each fUML RI log event to a callback the instant it fires — the RI's own {@code
 * fuml.Debug} class logs through Apache Commons Logging, which this project's dependency set
 * backs with log4j (confirmed directly against the ADR-005 spike's log output). Attaching a
 * custom {@link org.apache.log4j.Appender} is less invasive than capturing {@code System.out},
 * and it's *why* T-P1.4-01's "streams incrementally, not a terminal blob" is actually true here:
 * {@link #append} calls the sink synchronously, from inside the RI's own execution call.
 *
 * <p>Not thread-safe across concurrent executions — attached/detached around one synchronous
 * {@code execute()} call on the gRPC request-handling thread, matching this codebase's existing
 * single-user/single-request assumption (e.g. the canvas's Edit Mode) rather than solving
 * per-request isolation, which is out of scope for this pass.
 */
final class TraceStreamingAppender extends AppenderSkeleton {
    interface Sink {
        void onEvent(String kind, String activityName, String actionName, String detail);
    }

    private final Sink sink;

    TraceStreamingAppender(Sink sink) {
        this.sink = sink;
    }

    @Override
    protected void append(LoggingEvent event) {
        String message = String.valueOf(event.getMessage());
        String kind = "log";
        if (message.startsWith("Execute")) {
            kind = "execute";
        } else if (message.startsWith("Fire")) {
            kind = "fire";
        }
        sink.onEvent(kind, extractField(message, "activity="), extractField(message, "action="), message);
    }

    /**
     * Best-effort extraction from the RI's own "Execute activity=X" / "Fire activity=X action=Y"
     * message shape — a light parse, not a hard dependency: {@code detail} always carries the
     * full raw message regardless, so nothing is lost if a message doesn't match this shape.
     */
    private static String extractField(String message, String key) {
        int start = message.indexOf(key);
        if (start < 0) {
            return "";
        }
        start += key.length();
        int end = message.indexOf(' ', start);
        return end < 0 ? message.substring(start) : message.substring(start, end);
    }

    @Override
    public boolean requiresLayout() {
        return false;
    }

    @Override
    public void close() {
        // No resources held beyond the sink reference itself.
    }
}
