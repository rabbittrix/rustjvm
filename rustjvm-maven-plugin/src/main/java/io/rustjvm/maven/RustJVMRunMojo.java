package io.rustjvm.maven;

import org.apache.maven.plugin.MojoExecutionException;
import org.apache.maven.plugins.annotations.Mojo;

import java.util.List;

/**
 * {@code mvn rustjvm:run} — development mode with LiveRust hot reload.
 * Blocks, serving HTTP and watching sources, until interrupted.
 */
@Mojo(name = "run")
public class RustJVMRunMojo extends AbstractRustJVMMojo {

    /** Argument list for the run goal. Package-visible for tests. */
    List<String> runArgs() {
        return List.of("run", "--src", sourceDir.getPath(), "--port", String.valueOf(port));
    }

    @Override
    public void execute() throws MojoExecutionException {
        int exit = runCli(runArgs());
        if (exit != 0) {
            throw new MojoExecutionException("rustjvm run exited with code " + exit);
        }
    }
}
