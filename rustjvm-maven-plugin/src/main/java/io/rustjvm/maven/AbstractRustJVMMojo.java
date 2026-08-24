package io.rustjvm.maven;

import org.apache.maven.plugin.AbstractMojo;
import org.apache.maven.plugin.MojoExecutionException;
import org.apache.maven.plugins.annotations.Parameter;

import java.io.File;
import java.util.ArrayList;
import java.util.List;

/**
 * Shared machinery for all RustJVM goals: locate the {@code rustjvm} CLI,
 * assemble argument lists (pure and unit-testable), and run subprocesses.
 */
abstract class AbstractRustJVMMojo extends AbstractMojo {

    // Field initializers mirror the @Parameter defaults so plain unit tests
    // (which bypass Maven's injection) see the same values as a real build.

    /** Port for development mode. */
    @Parameter(property = "port", defaultValue = "8080")
    protected int port = 8080;

    /** Java source tree served by RustJVM. */
    @Parameter(property = "rustjvm.sourceDir", defaultValue = "${project.basedir}/src/main/java")
    protected File sourceDir = new File("src/main/java");

    /** The rustjvm CLI executable (must be on PATH or an absolute path). */
    @Parameter(property = "rustjvm.executable", defaultValue = "rustjvm")
    protected String executable = "rustjvm";

    /** Builds the full command line for a goal. Pure: no side effects. */
    List<String> buildCommand(List<String> args) {
        List<String> cmd = new ArrayList<>();
        cmd.add(executable);
        cmd.addAll(args);
        return cmd;
    }

    /** Runs rustjvm with the given arguments, inheriting this process's IO. */
    int runCli(List<String> args) throws MojoExecutionException {
        List<String> cmd = buildCommand(args);
        getLog().info("Executing: " + String.join(" ", cmd));
        ProcessBuilder pb = new ProcessBuilder(cmd);
        pb.inheritIO();
        try {
            return pb.start().waitFor();
        } catch (java.io.IOException e) {
            throw new MojoExecutionException(
                "Failed to launch '" + executable + "'. Is the RustJVM CLI on your PATH? "
                    + "(cargo install --path rustjvm-cli)", e);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new MojoExecutionException("Interrupted while running rustjvm", e);
        }
    }
}
