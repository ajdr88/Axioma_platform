package org.axioma.fumlruntime;

import io.grpc.Status;
import io.grpc.stub.StreamObserver;
import java.io.ByteArrayOutputStream;
import java.io.PrintStream;
import org.apache.log4j.Logger;
import org.axioma.fumlruntime.proto.ExecuteRequest;
import org.axioma.fumlruntime.proto.FumlRuntimeGrpc;
import org.axioma.fumlruntime.proto.TraceEvent;
import org.modeldriven.fuml.test.builtin.environment.TestEnvironment;
import org.modeldriven.fuml.test.builtin.environment.TestSuite;

/**
 * P1.4 (roadmap: Behavioral Simulation) — the gRPC-facing side of the fUML RI, proven working by
 * the ADR-005 spike. This pass supports exactly one fixed activity ("HelloWorld2", the same one
 * the spike already executed) — no XMI/model transfer yet, that's alf-lite's job later. The point
 * of this slice is proving the sidecar/streaming/determinism mechanics T-P1.4-01 actually asks
 * about, not the pilot's real behavior.
 */
final class FumlRuntimeServiceImpl extends FumlRuntimeGrpc.FumlRuntimeImplBase {
    private static final String HELLO_WORLD_ACTIVITY = "HelloWorld2";

    @Override
    public void execute(ExecuteRequest request, StreamObserver<TraceEvent> responseObserver) {
        String activityName = request.getActivityName();
        if (!HELLO_WORLD_ACTIVITY.equals(activityName)) {
            responseObserver.onError(
                    Status.INVALID_ARGUMENT
                            .withDescription("unsupported activity_name: " + activityName
                                    + " (only '" + HELLO_WORLD_ACTIVITY + "' this pass)")
                            .asRuntimeException());
            return;
        }

        TraceStreamingAppender.Sink sink = (kind, actName, actionName, detail) -> {
            TraceEvent event = TraceEvent.newBuilder()
                    .setActivityName(actName.isEmpty() ? activityName : actName)
                    .setActionName(actionName)
                    .setKind(kind)
                    .setDetail(detail)
                    .build();
            // Called synchronously from inside the RI's own execution call below — this is what
            // makes the stream genuinely incremental, not collected-then-sent.
            responseObserver.onNext(event);
        };

        Logger debugLogger = Logger.getLogger("fuml.Debug");
        TraceStreamingAppender appender = new TraceStreamingAppender(sink);
        PrintStream originalOut = System.out;
        PrintStream capturingOut = new OutputCapturingPrintStream(originalOut, activityName, sink);
        try {
            debugLogger.addAppender(appender);
            System.setOut(capturingOut);

            // A fresh TestEnvironment per call, not a shared/cached one — T-P1.4-01 requires 100
            // runs to be identical, and reusing a stateful environment across calls is a real way
            // that guarantee could quietly break (confirmed by testing it, not assumed safe).
            TestEnvironment environment = new TestEnvironment();
            TestSuite suite = new TestSuite(environment);
            suite.testHelloWorld();

            responseObserver.onCompleted();
        } catch (Exception e) {
            responseObserver.onError(
                    Status.INTERNAL.withDescription(e.toString()).withCause(e).asRuntimeException());
        } finally {
            debugLogger.removeAppender(appender);
            System.setOut(originalOut);
        }
    }

    /** Captures the RI's direct {@code System.out} writes (e.g. "Hello World!" — the actual
     * program output, which does not go through the Debug/log4j path) as "output"-kind trace
     * events, forwarded the instant each write completes, while still passing everything through
     * to the real stdout for server-side debugging via container logs.
     *
     * <p>Overrides {@code print(String)}, not {@code println(String)}: decompiling the RI's
     * {@code StandardOutputChannelObject} (confirmed directly, javap) shows it calls
     * {@code System.out.print(text)} for the content and a separate no-arg
     * {@code System.out.println()} only for the trailing newline — so a {@code println(String)}
     * override is never invoked at all and silently drops every "output"-kind event. */
    private static final class OutputCapturingPrintStream extends PrintStream {
        private final String activityName;
        private final TraceStreamingAppender.Sink sink;
        private final PrintStream delegate;

        OutputCapturingPrintStream(PrintStream original, String activityName, TraceStreamingAppender.Sink sink) {
            super(new ByteArrayOutputStream(), true);
            this.activityName = activityName;
            this.sink = sink;
            this.delegate = original;
        }

        @Override
        public void print(String text) {
            delegate.print(text);
            sink.onEvent("output", activityName, "", text);
        }
    }
}
