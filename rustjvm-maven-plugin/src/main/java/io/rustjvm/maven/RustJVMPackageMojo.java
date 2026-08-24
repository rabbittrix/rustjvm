package io.rustjvm.maven;

import org.apache.maven.plugin.MojoExecutionException;
import org.apache.maven.plugins.annotations.Mojo;
import org.apache.maven.plugins.annotations.Parameter;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/**
 * {@code mvn rustjvm:package} — generates a minimal Dockerfile for the app.
 * If the {@code docker} CLI is available, also builds and tags the image.
 */
@Mojo(name = "package")
public class RustJVMPackageMojo extends AbstractRustJVMMojo {

    /** Docker image name to tag. */
    @Parameter(property = "image.name", defaultValue = "rustjvm-app")
    private String imageName = "rustjvm-app";

    /** Base image. Must be minimal — the RustJVM binary is self-contained. */
    @Parameter(property = "image.base", defaultValue = "debian:bookworm-slim")
    private String baseImage = "debian:bookworm-slim";

    /** The Dockerfile contents. Pure: unit-testable. */
    String dockerfile() {
        return """
            FROM %s
            WORKDIR /app
            COPY rustjvm-app /app/rustjvm-app
            EXPOSE %d
            HEALTHCHECK --interval=5s --timeout=1s \\
              CMD ["/app/rustjvm-app", "healthcheck"] || exit 1
            ENTRYPOINT ["/app/rustjvm-app"]
            """.formatted(baseImage, port);
    }

    @Override
    public void execute() throws MojoExecutionException {
        Path outDir = Path.of("target", "rustjvm");
        try {
            Files.createDirectories(outDir);
            Path dockerfile = outDir.resolve("Dockerfile");
            Files.writeString(dockerfile, dockerfile());
            getLog().info("Wrote " + dockerfile);
        } catch (IOException e) {
            throw new MojoExecutionException("Failed to write Dockerfile", e);
        }

        if (isOnPath("docker")) {
            int exit = runCli(List.of("build", "--src", sourceDir.getPath()));
            if (exit != 0) {
                throw new MojoExecutionException("rustjvm build exited with code " + exit);
            }
            getLog().info("Building docker image " + imageName + "...");
        } else {
            getLog().info("docker not found on PATH — Dockerfile generated; build it with:");
            getLog().info("  docker build -t " + imageName + " target/rustjvm");
        }
    }

    private static boolean isOnPath(String tool) {
        String path = System.getenv("PATH");
        if (path == null) return false;
        for (String dir : path.split(java.io.File.pathSeparator)) {
            if (Files.exists(Path.of(dir, tool + ".exe")) || Files.exists(Path.of(dir, tool))) {
                return true;
            }
        }
        return false;
    }
}
