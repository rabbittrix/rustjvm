package io.rustjvm.maven;

import org.apache.maven.plugin.MojoExecutionException;
import org.apache.maven.plugins.annotations.Mojo;

import java.util.List;

/**
 * {@code mvn rustjvm:build} — validates the whole tree (routes compile, the
 * DI graph wires) and writes a build manifest to target/rustjvm/.
 */
@Mojo(name = "build")
public class RustJVMBuildMojo extends AbstractRustJVMMojo {

    List<String> buildArgs() {
        return List.of("build", "--src", sourceDir.getPath());
    }

    @Override
    public void execute() throws MojoExecutionException {
        int exit = runCli(buildArgs());
        if (exit != 0) {
            throw new MojoExecutionException("rustjvm build exited with code " + exit);
        }
    }
}
