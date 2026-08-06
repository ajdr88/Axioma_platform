package org.axioma.fumlruntime;

import io.grpc.Server;
import io.grpc.netty.shaded.io.grpc.netty.NettyServerBuilder;

/** P1.4 sidecar entry point — `FUML_RUNTIME_PORT` env var, default 50051 (the gRPC convention). */
public final class FumlRuntimeServer {
    private FumlRuntimeServer() {}

    public static void main(String[] args) throws Exception {
        int port = Integer.parseInt(System.getenv().getOrDefault("FUML_RUNTIME_PORT", "50051"));
        Server server = NettyServerBuilder.forPort(port)
                .addService(new FumlRuntimeServiceImpl())
                .build()
                .start();
        System.out.println("fuml-runtime listening on port " + port);
        Runtime.getRuntime().addShutdownHook(new Thread(server::shutdown));
        server.awaitTermination();
    }
}
