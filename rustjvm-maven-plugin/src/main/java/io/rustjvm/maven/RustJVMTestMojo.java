package io.rustjvm.maven;

import org.apache.maven.plugin.MojoExecutionException;
import org.apache.maven.plugins.annotations.Mojo;

import java.util.List;

/**
 * {@code mvn rustjvm:test} — boots the app on the RustJVM runtime and runs
 * the project's HTTP-level checks against it. (JUnit-on-RustJVM is on the
 * roadmap; today this goal validates boot + route table integrity.)
 */
@Mojo(name = "test")
public class RustJVMTestMojo extends AbstractRustJVMMojo {

    List<String> testArgs() {
        return List.of("routes", "--src", sourceDir.getPath());
    }

    @Override
    public void execute() throws MojoExecutionException {
        int exit = runCli(testArgs());
        if (exit != 0) {
            throw new MojoExecutionException("rustjvm test exited with code " + exit);
        }
    }
}
